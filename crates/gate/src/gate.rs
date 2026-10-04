use crate::cfg::{
    RuntimeConfig,
    graph::{CompileError, HttpGraph, ServiceNode},
};
use ace::PolicyEngine;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub type DynLoadBalancer = Arc<dyn lb::LoadBalancer + Send + Sync>;

pub struct PolicySnapshot {
    pub engine: PolicyEngine,
    pub revision: String,
}

impl PolicySnapshot {
    pub fn new(engine: PolicyEngine, revision: String) -> Self {
        Self { engine, revision }
    }
}

impl Default for PolicySnapshot {
    fn default() -> Self {
        Self {
            engine: PolicyEngine::new(),
            revision: "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                .to_string(),
        }
    }
}

/// Core data for one configuration generation. The application adds its
/// transports and version before publishing the complete snapshot.
pub struct Runtime {
    pub graph: HttpGraph,
    pub balancers: HashMap<String, DynLoadBalancer>,
    pub limiter: lim::Limiter,
    probes: Mutex<lb::HealthCheck>,
}

impl Runtime {
    pub fn prepare(graph: HttpGraph, limiter: lim::Limiter) -> Result<Self, CompileError> {
        let mut balancers = HashMap::new();
        let mut upstream_balancers = HashMap::new();
        let probes = lb::HealthCheck::new();
        for (name, service) in &graph.services {
            let ServiceNode::LoadBalancer { upstream, .. } = service else {
                continue;
            };
            let balancer = if let Some(balancer) = upstream_balancers.get(upstream) {
                Arc::clone(balancer)
            } else {
                let upstream_node = graph.upstreams.get(upstream).ok_or_else(|| {
                    CompileError::new(
                        format!("http.services.{name}.upstream"),
                        "referenced upstream not found in compiled graph",
                    )
                })?;
                let urls = upstream_node
                    .targets
                    .iter()
                    .map(|target| target.url.clone())
                    .collect::<Vec<_>>();
                let balancer = upstream_node.load_balancer.builder_from_urls(&urls);
                if let Some(interval) = upstream_node.load_balancer.probe_interval().map_err(
                    |message| {
                        CompileError::new(
                            format!(
                                "http.upstreams.{upstream}.load_balancer.liveness_probe.interval"
                            ),
                            message,
                        )
                    },
                )? {
                    probes.register(interval, balancer.clone());
                }
                upstream_balancers.insert(upstream.clone(), balancer.clone());
                balancer
            };
            balancers.insert(name.clone(), balancer);
        }
        // Preparing a candidate never starts or stops tasks in the active generation.
        Ok(Runtime {
            graph,
            balancers,
            limiter,
            probes: Mutex::new(probes),
        })
    }
    pub fn start_probes(&self) {
        self.probes.lock().expect("Probe lock poisoned").run();
    }

    pub fn stop_probes(&self) {
        self.probes.lock().expect("Probe lock poisoned").stop();
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.probes.get_mut().expect("Probe lock poisoned").stop();
    }
}

pub struct RuntimeBuilder {
    store: Arc<lim::State>,
    clock: Arc<lim::CachedClock>,
}

impl RuntimeBuilder {
    pub fn new(store: Arc<lim::State>) -> Self {
        Self {
            store,
            clock: lim::CachedClock::new(),
        }
    }

    pub fn prepare(&self, config: &RuntimeConfig) -> Result<Runtime, CompileError> {
        let graph = config.compiled.http.clone();
        let mut limiter = lim::Limiter::new();
        for limit in config.limits() {
            limiter.add_limit(
                limit.name.clone(),
                limit
                    .clone()
                    .build(self.store.clone(), self.clock.clone())?,
            );
        }
        Runtime::prepare(graph, limiter)
    }
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::RuntimeBuilder;
    use crate::cfg::RuntimeConfig;
    use std::sync::Arc;

    #[test]
    fn prepares_leaf_balancers_and_limits_without_activation() {
        let config = RuntimeConfig::from_yaml_str(
            r#"
schema: stargate/v1
ingress:
  limit: ingress
  timeout: 250ms
limits:
  ingress:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 100ms
  default:
    strategy: gcra
    params:
      max_burst: 3
      replenish_1_per: 500ms
http:
  upstreams:
    reports:
      targets:
        - url: http://localhost:4042
      load_balancer:
        strategy: round_robin
        liveness_probe:
          path: /health
          interval: 1s
  services:
    reports:
      kind: load_balancer
      upstream: reports
  routers:
    reports:
      match:
        path:
          prefix: /api/reports
      service: reports
"#,
        )
        .unwrap();
        // No Tokio runtime: preparation must not spawn probe tasks.
        let runtime = RuntimeBuilder::new(Arc::new(lim::State::new()))
            .prepare(&config)
            .unwrap();
        assert_eq!(runtime.graph.routers.len(), 1);
        assert!(runtime.balancers.contains_key("reports"));
        assert!(runtime.limiter.has_limit("default"));
    }

    #[test]
    fn invalid_limits_and_probe_intervals_fail_before_runtime_construction() {
        for (burst, duration) in [(0, "1s"), (1, "0s"), (1, "invalid"), (u32::MAX, "1000000d")] {
            let yaml = format!(
                "schema: stargate/v1\ningress:\n  limit: ingress\n  timeout: 250ms\nlimits:\n  ingress:\n    strategy: gcra\n    params:\n      max_burst: 100\n      replenish_1_per: 100ms\n  broken:\n    strategy: gcra\n    params:\n      max_burst: {burst}\n      replenish_1_per: {duration}\n"
            );
            let error = RuntimeConfig::from_yaml_str(&yaml).unwrap_err();
            assert!(error.to_string().contains("limits.broken"));
        }
        for interval in ["0s", "bad", "9223372036854775807s"] {
            let yaml = format!(
                "schema: stargate/v1\ningress:\n  limit: ingress\n  timeout: 250ms\nlimits:\n  ingress:\n    strategy: gcra\n    params:\n      max_burst: 100\n      replenish_1_per: 100ms\nhttp:\n  upstreams:\n    broken:\n      targets:\n        - url: http://localhost:1\n      load_balancer:\n        strategy: round_robin\n        liveness_probe:\n          path: /health\n          interval: {interval}\n"
            );
            assert!(
                RuntimeConfig::from_yaml_str(&yaml)
                    .unwrap_err()
                    .to_string()
                    .contains("liveness_probe.interval")
            );
        }
    }
}
