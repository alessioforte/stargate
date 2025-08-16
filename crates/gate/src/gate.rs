use crate::config::{Config, LoadBalancer, Service as Svc};
use crate::trie::{RouteNode, Service, TriePath};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::{collections::HashMap, path::Path, sync::Arc, thread, time::Duration};
use tokio::{runtime::Runtime, sync::RwLock};

#[derive(Clone)]
pub struct Gate {
    file_path: String,
    liveness_probe: Arc<RwLock<lb::HealthCheck>>,
    pub config: Arc<RwLock<TriePath>>,
}

impl Gate {
    pub fn new(file_path: String) -> Self {
        let trie = TriePath::new();
        let config = Arc::new(RwLock::new(trie));
        let liveness_probe = Arc::new(RwLock::new(lb::HealthCheck::new()));
        Self {
            file_path,
            config,
            liveness_probe,
        }
    }

    pub fn build(mut self) -> Self {
        let config = self.from_file();
        let trie = self.create_trie(&config.services);
        let config = Arc::new(RwLock::new(trie));
        self.config = config;
        self
    }

    pub async fn update_config(&mut self) {
        let liveness_prove = self.liveness_probe.clone();
        let mut liveness_probe = liveness_prove.write().await;
        liveness_probe.stop();

        let config = self.from_file();
        let trie = self.create_trie(&config.services);
        let mut config = self.config.write().await;
        *config = trie;
        log::info!("Gate configuration updated");
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
                connect_timeout: service.connect_timeout,
                auth_required: service.auth_required,
                name: service.name.clone(),
                path: service.path.clone(),
                lb: Some(*lb),
                routes,
            };

            trie.insert(&protocol, &service.path, node);
        }

        liveness_probe.run();
        self.liveness_probe = Arc::new(RwLock::new(liveness_probe));

        trie
    }

    fn from_file(&self) -> Config {
        let file_path = &self.file_path;
        if !Path::new(file_path).exists() {
            log::info!("Creating gate configuration yaml file");
            let config = Config::default();
            let config_str = serde_yml::to_string(&config).expect("Unable to serialize config");
            std::fs::write(file_path, config_str).expect("Unable to write config file");
            return config;
        }
        let file = std::fs::read_to_string(file_path).expect("Unable to read config file");
        let config: Config = serde_yml::from_str(&file).expect("Unable to parse config file");
        config
    }

    pub fn to_file(&self, cfg: &Config) {
        let config_str = serde_yml::to_string(cfg).expect("Unable to serialize config");
        std::fs::write(&self.file_path, config_str).expect("Unable to write config file");
    }

    pub fn watch_file(&self) {
        let mut gate = self.clone();
        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                log::info!("Watching Gate configuration file");
                let (tx, rx) = std::sync::mpsc::channel();
                let mut debouncer = new_debouncer(Duration::from_secs(0), tx).unwrap();
                debouncer
                    .watcher()
                    .watch(Path::new(&gate.file_path), RecursiveMode::Recursive)
                    .unwrap();

                for rs in rx {
                    match rs {
                        Ok(events) => {
                            for _e in events.iter() {
                                gate.update_config().await;
                            }
                        }
                        Err(e) => log::error!("Error: {:?}", e),
                    }
                }
            });
        });
    }
}
