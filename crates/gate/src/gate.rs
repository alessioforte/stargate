use crate::cfg::access_control::AccessControl;
use crate::cfg::{Config, limit::Limit, load_balancer::LoadBalancer, service::Service as Svc};
use crate::trie::{RouteNode, Service, TriePath};
use ace::PolicyEngine;
use arc_swap::ArcSwap;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Gate {
    store: Arc<lim::State>,
    liveness_probe: Arc<Mutex<lb::HealthCheck>>,
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
        Self {
            store,
            services,
            liveness_probe,
            policy_engine,
            limiter,
        }
    }

    pub fn build(self, config: &Config) -> Self {
        self.build_service(&config.services)
            .build_policy_engine(&config.access_control)
            .build_limiter(&config.limits)
    }

    fn build_service(mut self, svc: &Vec<Svc>) -> Self {
        let trie = self.create_trie(&svc);
        let services = Arc::new(ArcSwap::new(Arc::new(trie)));
        self.services = services;
        self
    }

    fn build_policy_engine(mut self, ac: &Option<AccessControl>) -> Self {
        let mut pe = PolicyEngine::new();
        if let Some(ac) = ac.clone() {
            if ac.policies_path.is_some() {
                let file_path = ac.policies_path.unwrap();
                let content =
                    std::fs::read_to_string(file_path).expect("Unable to read policy file");
                pe.parse_file(&content)
                    .expect("Unable to parse policy file");
            }
        }
        let policy_engine = Arc::new(ArcSwap::new(Arc::new(pe)));
        self.policy_engine = policy_engine;
        self
    }

    fn build_limiter(mut self, limits: &Option<Vec<Limit>>) -> Self {
        if let Some(limits) = limits.clone() {
            let mut limiter = lim::Limiter::new();
            for item in limits {
                let name = item.name.clone();
                let state = Arc::clone(&self.store);
                let limit = item.build(state);
                limiter.add_limit(name, limit);
            }
            self.limiter = Arc::new(ArcSwap::new(Arc::new(limiter)));
        }
        self
    }

    pub async fn update_config(&mut self, config: &Config) {
        self.update_services(&config.services).await;
        self.update_policy_engine(&config.access_control).await;
        self.update_limiter(&config.limits).await;
        log::info!("Gate configuration updated");
    }

    pub async fn update_services(&mut self, services: &Vec<Svc>) {
        // Stop the old liveness probe
        {
            let mut probe = self.liveness_probe.lock().unwrap();
            probe.stop();
        }

        let trie = self.create_trie(&services);
        self.services.store(Arc::new(trie));

        log::info!("Gate services updated");
    }

    pub async fn update_policy_engine(&mut self, ac: &Option<AccessControl>) {
        let mut pe = PolicyEngine::new();
        if let Some(ac) = ac.clone() {
            if ac.policies_path.is_some() {
                let file_path = ac.policies_path.unwrap();
                let content =
                    std::fs::read_to_string(file_path).expect("Unable to read policy file");
                pe.parse_file(&content)
                    .expect("Unable to parse policy file");
            }
        }
        self.policy_engine.store(Arc::new(pe));

        log::info!("Gate policy engine updated");
    }

    pub async fn update_limiter(&mut self, limits: &Option<Vec<Limit>>) {
        if let Some(limits) = limits.clone() {
            let mut limiter = lim::Limiter::new();
            for item in limits {
                let name = item.name.clone();
                let state = Arc::clone(&self.store);
                let limit = item.build(state);
                limiter.add_limit(name, limit);
            }
            self.limiter.store(Arc::new(limiter));
        }

        log::info!("Gate limiter updated");
    }

    fn create_trie(&mut self, services: &Vec<Svc>) -> TriePath {
        let mut trie = TriePath::new();
        let mut liveness_probe = lb::HealthCheck::new();

        for service in services {
            let protocol = service
                .protocol
                .clone()
                .unwrap_or_else(|| "http".to_string());

            let lb_strategy = match service.load_balancer.clone() {
                Some(lb) => lb,
                None => LoadBalancer::default(),
            };

            let lb = lb_strategy.builder(service.protocol.clone(), &service.endpoints);
            if lb_strategy.liveness_probe.is_some() {
                let health_check = lb_strategy.liveness_probe.unwrap();
                let i = health_check.interval.unwrap_or("5s".to_string());
                let interval = tools::parse_duration(&i).unwrap_or(chrono::Duration::seconds(5));
                let lb = Arc::clone(&lb);
                liveness_probe.register(interval, lb);
            }

            let mut routes = None;

            if let Some(service_routes) = &service.routes {
                let mut map: HashMap<String, matchit::Router<RouteNode>> = HashMap::new();
                for r in service_routes {
                    let route = RouteNode {
                        auth_required: r.auth_required.unwrap_or(false),
                        resource: r.resource.clone(),
                    };
                    let router = map.get(&r.method);
                    if router.is_none() {
                        let mut router = matchit::Router::new();
                        router.insert(&r.path, route).unwrap();
                        map.insert(r.method.clone(), router);
                    } else {
                        map.entry(r.method.clone()).and_modify(|router| {
                            router.insert(&r.path, route).unwrap();
                        });
                    }
                }
                routes = Some(map);
            }

            let node = Service {
                auth_required: service.auth_required,
                resource: service.resource.clone(),
                name: service.name.clone(),
                path: service.path.clone(),
                lb: Some(*lb),
                routes,
            };

            trie.insert(&protocol, &service.path, node);
        }

        liveness_probe.run();
        self.liveness_probe = Arc::new(Mutex::new(liveness_probe));

        trie
    }
}
