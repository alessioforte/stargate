use crate::cfg::{
    Limit, RuntimeConfig,
    graph::{CompileError, HttpGraph, ServiceNode},
};
use ace::PolicyEngine;
use arc_swap::ArcSwap;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tracing::{error, info};

type DynLoadBalancer = Arc<dyn lb::LoadBalancer + Send + Sync>;

#[derive(Clone)]
pub struct Gate {
    store: Arc<lim::State>,
    liveness_probe: Arc<Mutex<lb::HealthCheck>>,
    pub clock: Arc<lim::CachedClock>,
    pub http_graph: Arc<ArcSwap<HttpGraph>>,
    pub http_balancers: Arc<ArcSwap<HashMap<String, DynLoadBalancer>>>,
    pub policy_engine: Arc<ArcSwap<PolicyEngine>>,
    pub limiter: Arc<ArcSwap<lim::Limiter>>,
}

impl Gate {
    pub fn new(store: Arc<lim::State>) -> Self {
        let http_graph = Arc::new(ArcSwap::from_pointee(HttpGraph::default()));
        let http_balancers = Arc::new(ArcSwap::from_pointee(HashMap::new()));
        let liveness_probe = Arc::new(Mutex::new(lb::HealthCheck::new()));
        let policy_engine = Arc::new(ArcSwap::from_pointee(PolicyEngine::new()));
        let limiter = Arc::new(ArcSwap::from_pointee(lim::Limiter::new()));
        let clock = lim::CachedClock::new();
        Self {
            store,
            clock,
            http_graph,
            http_balancers,
            liveness_probe,
            policy_engine,
            limiter,
        }
    }

    pub fn build(self, config: &RuntimeConfig, policies_path: &str) -> Self {
        self.build_service(config)
            .build_policy_engine(policies_path)
            .build_limiter(config.limits())
    }

    fn build_service(mut self, config: &RuntimeConfig) -> Self {
        self.apply_runtime_config(config)
            .expect("Unable to build gateway runtime config");
        self
    }

    fn build_policy_engine(mut self, policies_path: &str) -> Self {
        let pe = Self::load_policy_engine(policies_path);
        self.policy_engine = Arc::new(ArcSwap::new(Arc::new(pe)));
        self
    }

    fn build_limiter(mut self, limits: &[Limit]) -> Self {
        let limiter = Self::create_limiter(limits, &self.store, &self.clock);
        self.limiter = Arc::new(ArcSwap::new(Arc::new(limiter)));
        self
    }

    pub async fn update_config(&mut self, config: &RuntimeConfig) {
        if let Err(error) = self.apply_runtime_config(config) {
            error!(%error, "Unable to update gateway services from runtime config");
            return;
        }

        self.update_limiter(config.limits()).await;
        info!("Gate configuration updated");
    }

    fn apply_runtime_config(&mut self, config: &RuntimeConfig) -> Result<(), CompileError> {
        {
            let mut probe = self.liveness_probe.lock().unwrap();
            probe.stop();
        }

        self.http_graph
            .store(Arc::new(config.compiled.http.clone()));
        let balancers = self.create_balancers_from_graph(&config.compiled.http)?;
        self.http_balancers.store(Arc::new(balancers));
        info!("Gate services updated");
        Ok(())
    }

    pub async fn update_policy_engine(&self, policies_path: &str) {
        let pe = Self::load_policy_engine(policies_path);
        self.policy_engine.store(Arc::new(pe));
        info!("Gate policy engine updated");
    }

    pub async fn update_limiter(&mut self, limits: &[Limit]) {
        let limiter = Self::create_limiter(limits, &self.store, &self.clock);
        self.limiter.store(Arc::new(limiter));
        info!("Gate limiter updated");
    }

    fn load_policy_engine(policies_path: &str) -> PolicyEngine {
        let mut pe = PolicyEngine::new();
        let content = match std::fs::read_to_string(policies_path) {
            Ok(c) => c,
            Err(e) => {
                error!("Unable to read policy file '{}': {}", policies_path, e);
                return pe;
            }
        };
        if let Err(e) = pe.parse_file(&content) {
            error!("Unable to parse policy file '{}': {}", policies_path, e);
        }
        pe
    }

    fn create_limiter(
        limits: &[Limit],
        store: &Arc<lim::State>,
        clock: &Arc<lim::CachedClock>,
    ) -> lim::Limiter {
        let mut limiter = lim::Limiter::new();
        for item in limits {
            let name = item.name.clone();
            let limit = item.clone().build(Arc::clone(store), Arc::clone(clock));
            limiter.add_limit(name, limit);
        }
        limiter
    }

    fn create_balancers_from_graph(
        &mut self,
        graph: &HttpGraph,
    ) -> Result<HashMap<String, DynLoadBalancer>, CompileError> {
        let mut balancers = HashMap::new();
        let mut upstream_balancers = HashMap::new();
        let mut liveness_probe = lb::HealthCheck::new();
        for (service_name, service) in &graph.services {
            let ServiceNode::LoadBalancer { upstream, .. } = service else {
                continue;
            };

            let lb = if let Some(lb) = upstream_balancers.get(upstream) {
                Arc::clone(lb)
            } else {
                let upstream_node = graph.upstreams.get(upstream).ok_or_else(|| {
                    CompileError::new(
                        format!("http.services.{}.upstream", service_name),
                        "referenced upstream not found in compiled graph",
                    )
                })?;

                let urls = upstream_node
                    .targets
                    .iter()
                    .map(|target| target.url.clone())
                    .collect::<Vec<_>>();
                let lb = upstream_node.load_balancer.builder_from_urls(&urls);

                if let Some(ref health_check) = upstream_node.load_balancer.liveness_probe {
                    let interval = health_check.interval.as_deref().unwrap_or("5s");
                    let interval =
                        tools::parse_duration(interval).unwrap_or(chrono::Duration::seconds(5));
                    liveness_probe.register(interval, Arc::clone(&lb));
                }

                upstream_balancers.insert(upstream.clone(), Arc::clone(&lb));
                lb
            };

            balancers.insert(service_name.clone(), lb);
        }

        liveness_probe.run();
        self.liveness_probe = Arc::new(Mutex::new(liveness_probe));

        Ok(balancers)
    }
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::Gate;
    use crate::cfg::RuntimeConfig;
    use std::sync::Arc;

    #[test]
    fn v2_runtime_builds_load_balancers_for_leaf_services() {
        let config = RuntimeConfig::from_yaml_str(
            r#"
schema: stargate/v2alpha1
limits:
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
        .expect("v2 config should load");

        let gate = Gate::new(Arc::new(lim::State::new())).build(&config, "/tmp/policies");
        assert_eq!(gate.http_graph.load().routers.len(), 1);
        assert!(gate.http_balancers.load().contains_key("reports"));
    }
}
