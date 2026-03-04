use crate::etc::store::use_store;
use actix_web::web::Data;
use gate::{Gate, cfg::Config};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use rustls::{ClientConfig, RootCertStore};
use rustls_pemfile::{certs, pkcs8_private_keys};
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
use tracing::{error, info};

fn get_config_dir() -> String {
    env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string())
}

fn get_config_path() -> String {
    let dir = get_config_dir();
    if !Path::new(&dir).exists() {
        std::fs::create_dir(&dir).expect("Unable to create config directory");
    }
    let filename = "config.yaml";
    format!("{}/{}", dir, filename)
}

pub fn get_policies_path() -> String {
    let dir = get_config_dir();
    let filename = "policies";
    format!("{}/{}", dir, filename)
}

fn get_config() -> Config {
    let config_file_path = get_config_path();
    gate::cfg::Config::from_file(&config_file_path)
}

pub fn init() -> Data<Gate> {
    let store = use_store();
    let config = get_config();
    let config_file_path = get_config_path();
    let policies_path = get_policies_path();

    let gate = Gate::new(Arc::new(store.clone())).build(&config, &policies_path);
    watch_config_file(&config_file_path, &gate);
    watch_policies_file(&policies_path, &gate);
    Data::new(gate)
}

static CONFIG_VERSION: AtomicU64 = AtomicU64::new(0);

fn watch_config_file(file_path: &str, gate: &Gate) {
    let file_path = file_path.to_string();
    let mut gate = gate.clone();
    thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            info!("Watching gate configuration file");
            let (tx, rx) = std::sync::mpsc::channel();

            let mut debouncer = new_debouncer(Duration::from_secs(0), tx).unwrap();
            debouncer
                .watcher()
                .watch(Path::new(&file_path), RecursiveMode::NonRecursive)
                .unwrap();

            for rs in rx {
                match rs {
                    Ok(events) => {
                        for _e in events.iter() {
                            CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
                            info!("Configuration file changed, reloading...");
                            let config = Config::from_file(&file_path);
                            gate.update_config(&config).await;
                        }
                    }
                    Err(e) => error!("Error: {:?}", e),
                }
            }
        });
    });
}

fn watch_policies_file(file_path: &str, gate: &Gate) {
    let file_path = file_path.to_string();
    let mut gate = gate.clone();
    thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            info!("Watching policies file");
            let (tx, rx) = std::sync::mpsc::channel();

            let mut debouncer = new_debouncer(Duration::from_secs(0), tx).unwrap();
            debouncer
                .watcher()
                .watch(Path::new(&file_path), RecursiveMode::NonRecursive)
                .unwrap();

            for rs in rx {
                match rs {
                    Ok(events) => {
                        for _e in events.iter() {
                            CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
                            info!("Policies file changed, reloading...");
                            gate.update_policy_engine(&file_path).await;
                        }
                    }
                    Err(e) => error!("Error: {:?}", e),
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

    let tls_config = if let Some(mtls) = cfg.mtls {
        Some(build_mtls(mtls))
    } else {
        None
    };

    for svc in &cfg.services {
        let timeout = svc.connect_timeout.unwrap_or(30);
        let mut client = awc::Client::builder()
            .timeout(Duration::from_secs(timeout))
            .finish();

        if let Some(tls_cfg) = &tls_config
            && svc.protocol == gate::protocol::Protocol::Https
        {
            let connector = awc::Connector::new().rustls_0_23(Arc::new(tls_cfg.clone()));
            client = awc::Client::builder()
                .timeout(Duration::from_secs(timeout))
                .connector(connector)
                .finish();
        }

        new_clients.insert(svc.name.clone(), client);
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

fn build_mtls(mtls: gate::cfg::mtls::MtlsConfig) -> ClientConfig {
    // read ca cert file
    let mut ca_cert_file = std::io::BufReader::new(
        std::fs::File::open(mtls.ca_cert_path).expect("Unable to open CA cert file"),
    );

    // load ca certs
    let ca_certs = certs(&mut ca_cert_file)
        .collect::<Result<Vec<_>, _>>()
        .expect("Unable to read CA certs");

    // create root cert store
    let mut root_store = RootCertStore::empty();
    // add ca certs to root store
    for cert in ca_certs {
        root_store
            .add(cert)
            .expect("Unable to add CA cert to root store");
    }

    // read client cert file
    let mut client_cert_file = std::io::BufReader::new(
        std::fs::File::open(mtls.client_cert_path).expect("Unable to open client cert file"),
    );
    let client_certs = certs(&mut client_cert_file)
        .collect::<Result<Vec<_>, _>>()
        .expect("Unable to read client certs");

    // read client key file
    let mut client_key_file = std::io::BufReader::new(
        std::fs::File::open(mtls.client_key_path).expect("Unable to open client key file"),
    );
    let mut client_keys = pkcs8_private_keys(&mut client_key_file)
        .collect::<Result<Vec<_>, _>>()
        .expect("Unable to read client private keys");

    if client_keys.is_empty() {
        panic!("No client private keys found");
    }

    // Configure TLS with mTLS
    let tls_config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_client_auth_cert(client_certs, client_keys.remove(0).into())
        .expect("Unable to create MTLS client config");

    tls_config
}
