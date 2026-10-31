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
    pub(crate) fn new(
        store: Arc<lim::State>,
        prepared: PreparedConfig,
        policies: PolicySnapshot,
    ) -> Result<Self, GatewayPreparationError> {
        let builder = RuntimeBuilder::new(store);
        let core = builder.prepare(&prepared.config)?;
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

    pub(crate) fn activate(
        &self,
        prepared: PreparedConfig,
    ) -> Result<u64, GatewayPreparationError> {
        self.publish(self.prepare_activation(prepared)?)
    }

    pub(crate) fn prepare_activation(
        &self,
        prepared: PreparedConfig,
    ) -> Result<PreparedActivation, GatewayPreparationError> {
        if prepared.config.compiled.runtime.budgets != self.resources.budgets {
            return Err(GatewayPreparationError::BudgetChange);
        }
        let core = self.builder.prepare(&prepared.config)?;
        configure_probes(&core, &prepared.transports);
        Ok(PreparedActivation { core, prepared })
    }

    pub(crate) fn publish(
        &self,
        activation: PreparedActivation,
    ) -> Result<u64, GatewayPreparationError> {
        let PreparedActivation { core, prepared } = activation;
        // Preparation finishes before touching the active generation or its probes.
        let _activation = self.activation.lock().expect("Activation lock poisoned");
        let version = self
            .runtime
            .load()
            .version
            .checked_add(1)
            .ok_or(GatewayPreparationError::VersionExhausted)?;
        let runtime = Arc::new(RuntimeSnapshot {
            core,
            ingress: prepared.config.compiled.ingress.clone(),
            settings: prepared.config.compiled.runtime.clone(),
            resources: self.resources.clone(),
            limit_store_health: self.limit_store_health.clone(),
            transports: prepared.transports,
            version,
            config_revision: prepared.config_revision,
        });
        let previous = self.runtime.swap(runtime.clone());
        runtime.core.start_probes();
        previous.core.stop_probes();
        Ok(version)
    }
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
