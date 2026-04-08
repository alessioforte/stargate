use crate::cfg::{Config, limit::Limit, service::Service as Svc};
use crate::trie::{RouteNode, Service, TriePath};
use ace::PolicyEngine;
use arc_swap::ArcSwap;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tracing::{error, info};

#[derive(Clone)]
pub struct Gate {
    store: Arc<lim::State>,
    liveness_probe: Arc<Mutex<lb::HealthCheck>>,
    pub clock: Arc<lim::CachedClock>,
    pub services: Arc<ArcSwap<TriePath>>,
    pub policy_engine: Arc<ArcSwap<PolicyEngine>>,
    pub limiter: Arc<ArcSwap<lim::Limiter>>,
}

impl Gate {
    pub fn new(store: Arc<lim::State>) -> Self {
        let trie = TriePath::new();
        let services = Arc::new(ArcSwap::from_pointee(trie));
        let liveness_probe = Arc::new(Mutex::new(lb::HealthCheck::new()));
        let policy_engine = Arc::new(ArcSwap::from_pointee(PolicyEngine::new()));
        let limiter = Arc::new(ArcSwap::from_pointee(lim::Limiter::new()));
        let clock = lim::CachedClock::new();
        Self {
            store,
            clock,
            services,
            liveness_probe,
            policy_engine,
            limiter,
        }
    }

    pub fn build(self, config: &Config, policies_path: &str) -> Self {
        self.build_service(&config.services)
            .build_policy_engine(policies_path)
            .build_limiter(&config.limits)
    }

    fn build_service(mut self, svc: &[Svc]) -> Self {
        let trie = self.create_trie(svc);
        self.services = Arc::new(ArcSwap::new(Arc::new(trie)));
        self
    }

    fn build_policy_engine(mut self, policies_path: &str) -> Self {
        let pe = Self::load_policy_engine(policies_path);
        self.policy_engine = Arc::new(ArcSwap::new(Arc::new(pe)));
        self
    }

    fn build_limiter(mut self, limits: &Option<Vec<Limit>>) -> Self {
        let limits = limits.as_deref().unwrap_or(&[]);
        let limiter = Self::create_limiter(limits, &self.store, &self.clock);
        self.limiter = Arc::new(ArcSwap::new(Arc::new(limiter)));
        self
    }

    pub async fn update_config(&mut self, config: &Config) {
        self.update_services(&config.services).await;
        self.update_limiter(&config.limits).await;
        info!("Gate configuration updated");
    }

    pub async fn update_services(&mut self, services: &[Svc]) {
        {
            let mut probe = self.liveness_probe.lock().unwrap();
            probe.stop();
        }

        let trie = self.create_trie(services);
        self.services.store(Arc::new(trie));

        info!("Gate services updated");
    }

    pub async fn update_policy_engine(&mut self, policies_path: &str) {
        let pe = Self::load_policy_engine(policies_path);
        self.policy_engine.store(Arc::new(pe));
        info!("Gate policy engine updated");
    }

    pub async fn update_limiter(&mut self, limits: &Option<Vec<Limit>>) {
        let limits = limits.as_deref().unwrap_or(&[]);
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

    fn create_trie(&mut self, services: &[Svc]) -> TriePath {
        let mut trie = TriePath::new();
        let mut liveness_probe = lb::HealthCheck::new();

        for service in services {
            let protocol = service.protocol.as_str();

            let lb_strategy = service.load_balancer.as_ref().cloned().unwrap_or_default();

            let lb = lb_strategy.builder(protocol, &service.endpoints);
            if let Some(ref health_check) = lb_strategy.liveness_probe {
                let i = health_check.interval.as_deref().unwrap_or("5s");
                let interval = tools::parse_duration(i).unwrap_or(chrono::Duration::seconds(5));
                liveness_probe.register(interval, Arc::clone(&lb));
            }

            let mut routes = None;

            if let Some(service_routes) = &service.routes {
                let mut map: HashMap<String, matchit::Router<RouteNode>> = HashMap::new();
                for r in service_routes {
                    let route = RouteNode {
                        auth_required: r.auth_required.unwrap_or(false),
                        resource: r.resource.clone(),
                        cost: r.cost,
                    };
                    map.entry(r.method.clone())
                        .or_insert_with(matchit::Router::new)
                        .insert(&r.path, route)
                        .unwrap();
                }
                routes = Some(map);
            }

            let node = Service {
                auth_required: service.auth_required,
                resource: service.resource.clone(),
                name: service.name.clone(),
                path: service.path.clone(),
                lb: Some(lb),
                cost: service.cost,
                routes,
            };

            trie.insert(protocol, &service.path, node);
        }

        liveness_probe.run();
        self.liveness_probe = Arc::new(Mutex::new(liveness_probe));

        trie
    }
}
