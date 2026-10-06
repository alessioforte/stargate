#[cfg(feature = "memory")]
use gate::cfg::Config;

pub(crate) fn runtime(config: gate::cfg::RuntimeConfig) -> std::sync::Arc<super::RuntimeSnapshot> {
    let prepared = super::super::config::prepare_runtime_config(config).unwrap();
    let core = gate::Runtime::prepare(prepared.config.compiled.http, lim::Limiter::new()).unwrap();
    std::sync::Arc::new(super::RuntimeSnapshot {
        core,
        ingress: prepared.config.compiled.ingress,
        resources: super::super::resources::ProcessResources::new(
            prepared.config.compiled.runtime.budgets,
        ),
        settings: prepared.config.compiled.runtime,
        transports: prepared.transports,
        version: 0,
    })
}

#[cfg(feature = "memory")]
pub(crate) fn gate(config: Config) -> super::Gate {
    super::Gate::new(
        std::sync::Arc::new(lim::State::new()),
        super::super::prepare_config(config).unwrap(),
        gate::PolicySnapshot::default(),
    )
    .unwrap()
}

#[cfg(feature = "memory")]
pub(crate) fn gate_with_limiter(config: Config, limiter: lim::Limiter) -> super::Gate {
    let gate = gate(config.clone());
    let prepared = super::super::prepare_config(config).unwrap();
    let core = gate::Runtime::prepare(prepared.config.compiled.http, limiter).unwrap();
    let runtime = std::sync::Arc::new(super::RuntimeSnapshot {
        core,
        ingress: prepared.config.compiled.ingress,
        settings: prepared.config.compiled.runtime,
        resources: gate.resources.clone(),
        transports: prepared.transports,
        version: 0,
    });
    gate.runtime.store(runtime.clone());
    runtime.core.start_probes();
    gate
}
