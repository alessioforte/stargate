use super::{
    config::{GatewayPreparationError, PreparedConfig},
    resources,
    transport::{HyperClient, PreparedTransport, PreparedTransports},
};
use arc_swap::ArcSwap;
use gate::{PolicySnapshot, RuntimeBuilder};
use std::sync::{Arc, Mutex};

pub struct RuntimeSnapshot {
    pub core: gate::Runtime,
    pub version: u64,
    pub ingress: gate::cfg::CompiledIngress,
    pub settings: gate::cfg::CompiledRuntimeSettings,
    pub resources: Arc<resources::ProcessResources>,
    transports: PreparedTransports,
}

impl RuntimeSnapshot {
    pub fn client(&self, service: &str) -> Option<HyperClient> {
        self.transport(service)
            .map(|transport| transport.http.clone())
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
}

impl Gate {
    pub(crate) fn new(
        store: Arc<lim::State>,
        prepared: PreparedConfig,
        policies: PolicySnapshot,
    ) -> Result<Self, GatewayPreparationError> {
        let builder = RuntimeBuilder::new(store);
        let core = builder.prepare(&prepared.config)?;
        let settings = prepared.config.compiled.runtime.clone();
        let resources = resources::ProcessResources::new(settings.budgets);
        let runtime = Arc::new(RuntimeSnapshot {
            core,
            ingress: prepared.config.compiled.ingress.clone(),
            settings,
            resources: resources.clone(),
            transports: prepared.transports,
            version: 0,
        });
        runtime.core.start_probes();
        Ok(Self {
            builder,
            resources,
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
        if prepared.config.compiled.runtime.budgets != self.resources.budgets {
            return Err(GatewayPreparationError::BudgetChange);
        }
        let core = self.builder.prepare(&prepared.config)?;
        // Serialize only activation: file reads, TLS and core construction all
        // finish before touching the active generation or its probes.
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
            transports: prepared.transports,
            version,
        });
        let previous = self.runtime.swap(runtime.clone());
        runtime.core.start_probes();
        previous.core.stop_probes();
        Ok(version)
    }
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(all(test, feature = "memory"))]
mod tests;
