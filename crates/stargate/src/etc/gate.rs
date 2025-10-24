use crate::etc::store::use_store;
use actix_web::web::Data;
use gate::{Gate, cfg::Config};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::{
    collections::HashMap,
    env,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};
use tokio::runtime::Runtime;

fn get_config_path() -> String {
    let path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
    let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    if !Path::new(&path).exists() {
        std::fs::create_dir(&path).expect("Unable to create config directory");
    }
    format!("{}/{}", path, filename)
}

fn get_config() -> Config {
    let config_file_path = get_config_path();
    gate::cfg::Config::from_file(&config_file_path)
}

pub fn init() -> Data<Gate> {
    let store = use_store();
    let config = get_config();
    let config_file_path = get_config_path();
    let gate = Gate::new(Arc::new(store.clone())).build(&config);
    watch_file(&config_file_path, &gate);
    Data::new(gate)
}

static CONFIG_VERSION: AtomicU64 = AtomicU64::new(0);

pub fn watch_file(file_path: &str, gate: &Gate) {
    let file_path = file_path.to_string();
    let mut gate = gate.clone();
    thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            log::info!("Watching Gate configuration file");
            let (tx, rx) = std::sync::mpsc::channel();

            let mut debouncer = new_debouncer(Duration::from_secs(0), tx).unwrap();
            debouncer
                .watcher()
                .watch(Path::new(&file_path), RecursiveMode::Recursive)
                .unwrap();

            for rs in rx {
                match rs {
                    Ok(events) => {
                        for _e in events.iter() {
                            CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
                            log::info!("Configuration file changed, reloading...");
                            let config = Config::from_file(&file_path);
                            gate.update_config(&config).await;
                        }
                    }
                    Err(e) => log::error!("Error: {:?}", e),
                }
            }
        });
    });
}

pub fn get_config_version() -> u64 {
    CONFIG_VERSION.load(Ordering::SeqCst)
}

struct LocalClients {
    version: u64,
    clients: HashMap<String, awc::Client>,
}

thread_local! {
    static CLIENT_POOL: std::cell::RefCell<Option<LocalClients>> = const { std::cell::RefCell::new(None) };
}

pub fn build_clients() -> HashMap<String, awc::Client> {
    let mut new_clients = HashMap::new();
    let cfg = get_config();
    for svc in &cfg.services {
        if let Some(timeout) = svc.connect_timeout {
            let client = awc::Client::builder()
                .timeout(Duration::from_millis(timeout as u64))
                .finish();
            new_clients.insert(svc.name.clone(), client);
        } else {
            let client = awc::Client::default();
            new_clients.insert(svc.name.clone(), client);
        }
    }
    new_clients
}

fn update_clients_if_needed() {
    let current_version = get_config_version();
    CLIENT_POOL.with(|pool| {
        let mut pool_ref = pool.borrow_mut();
        let needs_update = match &*pool_ref {
            Some(local_clients) => local_clients.version != current_version,
            None => true,
        };
        if needs_update {
            let new_clients = build_clients();
            *pool_ref = Some(LocalClients {
                version: current_version,
                clients: new_clients,
            });
        }
    });
}

pub fn get_client(service_name: &str) -> Option<awc::Client> {
    update_clients_if_needed();

    let mut client_opt = None;
    CLIENT_POOL.with(|pool| {
        if let Some(local_clients) = &*pool.borrow() {
            if let Some(client) = local_clients.clients.get(service_name) {
                client_opt = Some(client.clone());
            }
        }
    });
    client_opt
}
