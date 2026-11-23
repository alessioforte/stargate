use super::config::{GatewayPreparationError, PreparedConfig};
use crate::gate::{
    resources,
    transport::{HyperClient, PreparedTransport, PreparedTransports},
};
use arc_swap::ArcSwap;
use gate::{PolicySnapshot, RuntimeBuilder};
use std::sync::{Arc, Mutex};

pub struct RuntimeSnapshot {
    pub core: gate::Runtime,
    pub version: u64,
    pub config_revision: String,
    pub(crate) config_document_version: i64,
    pub ingress: gate::cfg::CompiledIngress,
    pub settings: gate::cfg::CompiledRuntimeSettings,
    pub resources: Arc<resources::ProcessResources>,
    pub(crate) limit_store_health: Arc<crate::gate::limits::LimitStoreHealth>,
    transports: PreparedTransports,
}

impl RuntimeSnapshot {
    #[cfg(test)]
    pub fn client(&self, service: &str) -> Option<HyperClient> {
        self.transport(service)
            .map(|transport| transport.http.clone())
    }

    pub fn client_for(
        &self,
        service: &str,
        grpc: bool,
    ) -> Result<HyperClient, crate::err::ErrorResponse> {
        use crate::err::{ErrorCode, ErrorResponse};
        let transport = self
            .transport(service)
            .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayHttpClientUnavailable))?;
        match grpc {
            false => Ok(transport.http.clone()),
            true => transport
                .grpc_http
                .clone()
                .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayGrpcHttp2Required)),
        }
    }

    pub fn transport(&self, service: &str) -> Option<&PreparedTransport> {
        self.transports.get(service)
    }
}

pub struct Gate {
    builder: RuntimeBuilder,
    runtime: ArcSwap<RuntimeSnapshot>,
    activation: Mutex<()>,
    pub policy_snapshot: ArcSwap<PolicySnapshot>,
    pub resources: Arc<resources::ProcessResources>,
    pub(crate) limit_store_health: Arc<crate::gate::limits::LimitStoreHealth>,
}

pub(crate) struct PreparedActivation {
    core: gate::Runtime,
    prepared: PreparedConfig,
}

impl Gate {
    pub(in crate::gate) async fn from_document(
        store: Arc<lim::State>,
        prepared: PreparedConfig,
        policies: PolicySnapshot,
    ) -> Result<Self, GatewayPreparationError> {
        let database = crate::data::db::service();
        let ledger = crate::gate::limits::SqlQuotaLedger::new(database.clone());
        let gate = Self::new(store, prepared, policies, Some(Arc::new(ledger)))?;
        let runtime = gate.snapshot();
        activate_quotas(
            &database,
            runtime.config_document_version,
            &runtime.core.limiter,
        )
        .await?;
        Ok(gate)
    }

    pub(crate) fn new(
        store: Arc<lim::State>,
        prepared: PreparedConfig,
        policies: PolicySnapshot,
        quota_ledger: Option<Arc<dyn lim::QuotaLedger>>,
    ) -> Result<Self, GatewayPreparationError> {
        let builder = RuntimeBuilder::new(store).with_quota_ledger(quota_ledger);
        let mut core = builder.prepare(&prepared.config)?;
        core.limiter
            .set_quota_config_version(prepared.config_document_version);
        configure_probes(&core, &prepared.transports);
        let settings = prepared.config.compiled.runtime.clone();
        let resources = resources::ProcessResources::new(settings.budgets);
        let limit_store_health = Arc::default();
        let runtime = Arc::new(RuntimeSnapshot {
            core,
            ingress: prepared.config.compiled.ingress.clone(),
            settings,
            resources: resources.clone(),
            limit_store_health: Arc::clone(&limit_store_health),
            transports: prepared.transports,
            version: 0,
            config_revision: prepared.config_revision,
            config_document_version: prepared.config_document_version,
        });
        runtime.core.start_probes();
        Ok(Self {
            builder,
            resources,
            limit_store_health,
            runtime: ArcSwap::from(runtime),
            activation: Mutex::new(()),
            policy_snapshot: ArcSwap::from_pointee(policies),
        })
    }

    pub fn snapshot(&self) -> Arc<RuntimeSnapshot> {
        self.runtime.load_full()
    }

    #[cfg(test)]
    pub(crate) fn activate(
        &self,
        prepared: PreparedConfig,
    ) -> Result<u64, GatewayPreparationError> {
        let version = prepared.config_document_version;
        self.publish(self.prepare_activation(prepared)?, version)
    }

    pub(crate) fn prepare_activation(
        &self,
        prepared: PreparedConfig,
    ) -> Result<PreparedActivation, GatewayPreparationError> {
        if prepared.config.compiled.runtime.budgets != self.resources.budgets {
            return Err(GatewayPreparationError::BudgetChange);
        }
        let core = self.builder.prepare(&prepared.config)?;
        Ok(PreparedActivation { core, prepared })
    }

    pub(super) async fn publish_document(
        self: &Arc<Self>,
        activation: PreparedActivation,
        version: i64,
    ) -> Result<u64, GatewayPreparationError> {
        let database = crate::data::db::service();
        let gate = self.clone();
        // Once publication starts, finish it even if the admin request ends.
        tokio::spawn(async move {
            activate_quotas(&database, version, &activation.core.limiter).await?;
            gate.publish(activation, version)
        })
        .await?
    }

    fn publish(
        &self,
        activation: PreparedActivation,
        config_document_version: i64,
    ) -> Result<u64, GatewayPreparationError> {
        let PreparedActivation {
            mut core,
            mut prepared,
        } = activation;
        // Preparation finishes before touching the active generation or its probes.
        let _activation = self.activation.lock().expect("Activation lock poisoned");
        let active = self.runtime.load();
        if config_document_version > 0 && active.config_document_version >= config_document_version
        {
            return Ok(active.version);
        }
        let version = active
            .version
            .checked_add(1)
            .ok_or(GatewayPreparationError::VersionExhausted)?;
        // A different generation may have been published since preparation.
        prepared.transports.reuse_from(&active.transports);
        core.limiter
            .set_quota_config_version(config_document_version);
        configure_probes(&core, &prepared.transports);
        let runtime = Arc::new(RuntimeSnapshot {
            core,
            ingress: prepared.config.compiled.ingress.clone(),
            settings: prepared.config.compiled.runtime.clone(),
            resources: self.resources.clone(),
            limit_store_health: self.limit_store_health.clone(),
            transports: prepared.transports,
            version,
            config_revision: prepared.config_revision,
            config_document_version,
        });
        let previous = self.runtime.swap(runtime.clone());
        runtime.core.start_probes();
        previous.core.stop_probes();
        Ok(version)
    }
}

async fn activate_quotas(
    database: &db::service::Service,
    version: i64,
    limiter: &lim::Limiter,
) -> Result<(), GatewayPreparationError> {
    let definitions: Vec<_> = limiter.durable_quota_definitions().collect();
    database
        .activate_quota_definitions(version, &definitions)
        .await
        .map_err(GatewayPreparationError::QuotaActivation)
}

fn configure_probes(core: &gate::Runtime, transports: &PreparedTransports) {
    let mut configured = std::collections::HashSet::new();
    for (service, node) in &core.graph.services {
        let gate::graph::ServiceNode::LoadBalancer { upstream, .. } = node else {
            continue;
        };
        if configured.insert(upstream)
            && let Some(client) = transports
                .get(service)
                .and_then(|transport| transport.probe_client.as_ref())
        {
            core.balancers[service].set_health_client(client.clone());
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(all(test, feature = "memory"))]
mod tests;
