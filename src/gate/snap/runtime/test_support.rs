#[cfg(feature = "memory")]
use gate::cfg::Config;

pub(crate) fn runtime(config: gate::cfg::RuntimeConfig) -> std::sync::Arc<super::RuntimeSnapshot> {
    let revision = db::ent::config_revision(&config.raw.to_yaml_string().unwrap());
    let prepared = super::super::config::prepare_runtime_config(config, revision).unwrap();
    let core = gate::Runtime::prepare(prepared.config.compiled.http, lim::Limiter::new()).unwrap();
    super::configure_probes(&core, &prepared.transports);
    std::sync::Arc::new(super::RuntimeSnapshot {
        core,
        limit_store_health: std::sync::Arc::default(),
        ingress: prepared.config.compiled.ingress,
        resources: crate::gate::resources::ProcessResources::new(
            prepared.config.compiled.runtime.budgets,
        ),
        settings: prepared.config.compiled.runtime,
        transports: prepared.transports,
        version: 0,
        config_revision: prepared.config_revision,
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
    super::configure_probes(&core, &prepared.transports);
    let runtime = std::sync::Arc::new(super::RuntimeSnapshot {
        core,
        ingress: prepared.config.compiled.ingress,
        settings: prepared.config.compiled.runtime,
        resources: gate.resources.clone(),
        limit_store_health: gate.limit_store_health.clone(),
        transports: prepared.transports,
        version: 0,
        config_revision: prepared.config_revision,
    });
    gate.runtime.store(runtime.clone());
    runtime.core.start_probes();
    gate
}
