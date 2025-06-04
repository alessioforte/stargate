use crate::config::{Config, LoadBalancerStrategy};
use crate::trie::{RouteNode, Service, TriePath};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct Gate {
    pub config: Arc<RwLock<TriePath>>,
}

impl Gate {
    pub fn init() -> Self {
        let trie = Self::create_trie();
        let config = Arc::new(RwLock::new(trie));
        Self { config }
    }

    pub async fn update_config(&self) {
        let trie = Self::create_trie();
        let mut config = self.config.write().await;
        *config = trie;
        log::info!("Gate configuration updated");
    }

    fn create_trie() -> TriePath {
        let mut trie = TriePath::new();
        let cfg = Config::from_file();
        for service in &cfg.services {
            let protocol = service
                .protocol
                .clone()
                .unwrap_or_else(|| "http".to_string());

            let lb_strategy = service
                .load_balancer
                .clone()
                .unwrap_or_else(|| LoadBalancerStrategy::default());

            let lb = lb_strategy.build(service);

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

        trie
    }

    pub fn watch_file(&self) {
        let gate = self.clone();
        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                log::info!("Watching Gate configuration file");
                let path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
                let filename =
                    env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
                let path = format!("{}/{}", path, filename);
                let (tx, rx) = std::sync::mpsc::channel();
                let mut debouncer = new_debouncer(Duration::from_secs(0), tx).unwrap();
                debouncer
                    .watcher()
                    .watch(Path::new(path.as_str()), RecursiveMode::Recursive)
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
