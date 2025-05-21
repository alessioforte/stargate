use crate::config::Config;
use crate::trie::TriePath;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::env;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct Gate {
    pub config: Arc<RwLock<TriePath>>,
}

impl Gate {
    pub fn init() -> Self {
        let cfg = Config::from_file();
        let mut trie = TriePath::new();
        for service in &cfg.services {
            // trie.insert(&service.path, service.clone());
            let protocol = service
                .uri
                .protocol
                .clone()
                .unwrap_or_else(|| "http".to_string());
            trie.insert(&protocol, &service.path, service.clone());
        }
        let config = Arc::new(RwLock::new(trie));
        Self { config }
    }

    pub async fn update_config(&self) {
        let new_config = Config::from_file();
        let mut trie = TriePath::new();
        for service in &new_config.services {
            // trie.insert(&service.path, service.clone());
            let protocol = service
                .uri
                .protocol
                .clone()
                .unwrap_or_else(|| "http".to_string());
            trie.insert(&protocol, &service.path, service.clone());
        }
        let mut config = self.config.write().await;
        log::info!("Updating config");
        *config = trie;
    }

    pub fn watch_file(&self) {
        let gate = self.clone();
        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                log::info!("Watching config file");
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
