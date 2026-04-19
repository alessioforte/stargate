use crate::etc::store::use_store;
use actix_web::web::Data;
use gate::{Gate, cfg::Config};
use notify::{EventKind, RecursiveMode, Watcher, event::ModifyKind};
use rustls::{ClientConfig, RootCertStore};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::{
    collections::HashMap,
    env,
    path::Path,
    sync::{
        Arc, OnceLock, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};
use tokio::runtime::Handle;
use tracing::{error, info};

fn get_config_dir() -> String {
    env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string())
}

fn get_config_path() -> String {
    let dir = get_config_dir();
    if !Path::new(&dir).exists() {
        std::fs::create_dir(&dir).expect("Unable to create config directory");
    }
    let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    format!("{}/{}", dir, filename)
}

pub fn get_policies_path() -> String {
    let dir = get_config_dir();
    let filename = "policies";
    let path = format!("{}/{}", dir, filename);
    if !Path::new(&path).exists() {
        std::fs::write(&path, "").expect("Unable to create policies file");
    }
    path
}

fn load_config() -> Config {
    let config_file_path = get_config_path();
    gate::cfg::Config::from_file(&config_file_path)
}

pub fn init() -> Data<Gate> {
    let store = use_store();
    let config = get_config();
    let config_file_path = get_config_path();
    let policies_path = get_policies_path();

    let gate = Gate::new(Arc::new(store.clone())).build(config.as_ref(), &policies_path);
    watch_config_file(&config_file_path, &gate);
    watch_policies_file(&policies_path, &gate);
    Data::new(gate)
}

static CONFIG_VERSION: AtomicU64 = AtomicU64::new(0);
static CONFIG_CACHE: OnceLock<RwLock<CachedConfig>> = OnceLock::new();

#[derive(Clone)]
struct CachedConfig {
    version: u64,
    config: Arc<Config>,
}

fn config_cache() -> &'static RwLock<CachedConfig> {
    CONFIG_CACHE.get_or_init(|| {
        RwLock::new(CachedConfig {
            version: 0,
            config: Arc::new(load_config()),
        })
    })
}

fn get_config() -> Arc<Config> {
    let cache = config_cache().read().expect("Config cache lock poisoned");
    cache.config.clone()
}

fn get_config_snapshot() -> CachedConfig {
    config_cache()
        .read()
        .expect("Config cache lock poisoned")
        .clone()
}

fn update_cached_config(config: Config) -> u64 {
    let mut cache = config_cache().write().expect("Config cache lock poisoned");
    cache.version += 1;
    cache.config = Arc::new(config);
    cache.version
}

/// Returns the full file content so change detection stays deterministic.
fn file_content(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

/// Returns true only for data-modification events (write/save),
/// filtering out access, open, metadata, and rename events.
fn is_write_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Modify(ModifyKind::Data(_)) | EventKind::Create(_)
    )
}

fn create_file_watcher(
    file_path: &str,
    label: &'static str,
    tx: std::sync::mpsc::Sender<notify::Result<notify::Event>>,
) -> Option<notify::RecommendedWatcher> {
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(watcher) => watcher,
        Err(error) => {
            error!(
                path = file_path,
                watcher = label,
                error = ?error,
                "Failed to initialize file watcher; live reload disabled"
            );
            return None;
        }
    };

    if let Err(error) = watcher.watch(Path::new(file_path), RecursiveMode::NonRecursive) {
        error!(
            path = file_path,
            watcher = label,
            error = ?error,
            "Failed to register file watcher; live reload disabled"
        );
        return None;
    }

    info!(
        path = file_path,
        watcher = label,
        "Watching file for changes"
    );
    Some(watcher)
}

fn watch_config_file(file_path: &str, gate: &Gate) {
    let file_path = file_path.to_string();
    let mut gate = gate.clone();
    let handle = Handle::current();
    thread::spawn(move || {
        handle.block_on(async {
            let (tx, rx) = std::sync::mpsc::channel();

            let mut last_content = file_content(&file_path);

            let Some(_watcher) = create_file_watcher(&file_path, "gate_config", tx) else {
                return;
            };

            for rs in rx {
                match rs {
                    Ok(event) => {
                        if !is_write_event(&event.kind) {
                            continue;
                        }
                        let current_content = file_content(&file_path);
                        if current_content == last_content {
                            continue;
                        }
                        last_content = current_content;

                        let config = Config::from_file(&file_path);
                        let config_version = update_cached_config(config.clone());
                        CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
                        info!(config_version, "Configuration file changed, reloading...");
                        gate.update_config(&config).await;
                    }
                    Err(e) => error!("Watch error: {:?}", e),
                }
            }
        });
    });
}

fn watch_policies_file(file_path: &str, gate: &Gate) {
    let file_path = file_path.to_string();
    let mut gate = gate.clone();
    let handle = Handle::current();
    thread::spawn(move || {
        handle.block_on(async {
            let (tx, rx) = std::sync::mpsc::channel();

            let mut last_content = file_content(&file_path);

            let Some(_watcher) = create_file_watcher(&file_path, "policies", tx) else {
                return;
            };

            for rs in rx {
                match rs {
                    Ok(event) => {
                        if !is_write_event(&event.kind) {
                            continue;
                        }
                        let current_content = file_content(&file_path);
                        if current_content == last_content {
                            continue;
                        }
                        last_content = current_content;

                        CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
                        info!("Policies file changed, reloading...");
                        gate.update_policy_engine(&file_path).await;
                    }
                    Err(e) => error!("Watch error: {:?}", e),
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
    streaming_clients: HashMap<String, awc::Client>,
}

thread_local! {
    static CLIENT_POOL: std::cell::RefCell<Option<LocalClients>> = const { std::cell::RefCell::new(None) };
}

struct BuiltClients {
    clients: HashMap<String, awc::Client>,
    streaming_clients: HashMap<String, awc::Client>,
}

fn service_has_streaming_route(svc: &gate::cfg::service::Service) -> bool {
    svc.routes
        .as_ref()
        .map(|rs| rs.iter().any(|r| r.streaming.is_some()))
        .unwrap_or(false)
}

fn build_clients(config: &Config) -> BuiltClients {
    let mut new_clients = HashMap::new();
    let mut new_streaming_clients = HashMap::new();

    let tls_config = if let Some(mtls) = &config.mtls {
        Some(build_mtls(mtls))
    } else {
        None
    };

    for svc in &config.services {
        let timeout = svc.connect_timeout.unwrap_or(30);
        let use_tls = tls_config.is_some() && svc.protocol == gate::protocol::Protocol::Https;

        let client = if use_tls
            && let Some(tls_cfg) = &tls_config
        {
            let connector = awc::Connector::new().rustls_0_23(Arc::new(tls_cfg.clone()));
            awc::Client::builder()
                .timeout(Duration::from_secs(timeout))
                .connector(connector)
                .finish()
        } else {
            awc::Client::builder()
                .timeout(Duration::from_secs(timeout))
                .finish()
        };
        new_clients.insert(svc.name.clone(), client);

        if service_has_streaming_route(svc) {
            let streaming_client = if use_tls
                && let Some(tls_cfg) = &tls_config
            {
                let connector = awc::Connector::new().rustls_0_23(Arc::new(tls_cfg.clone()));
                awc::Client::builder()
                    .disable_timeout()
                    .connector(connector)
                    .finish()
            } else {
                awc::Client::builder().disable_timeout().finish()
            };
            new_streaming_clients.insert(svc.name.clone(), streaming_client);
        }
    }
    BuiltClients {
        clients: new_clients,
        streaming_clients: new_streaming_clients,
    }
}

fn update_clients_if_needed() {
    let config_snapshot = get_config_snapshot();
    CLIENT_POOL.with(|pool| {
        let mut pool_ref = pool.borrow_mut();
        let needs_update = match &*pool_ref {
            Some(local_clients) => local_clients.version != config_snapshot.version,
            None => true,
        };
        if needs_update {
            let built = build_clients(config_snapshot.config.as_ref());
            *pool_ref = Some(LocalClients {
                version: config_snapshot.version,
                clients: built.clients,
                streaming_clients: built.streaming_clients,
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

pub fn get_streaming_client(service_name: &str) -> Option<awc::Client> {
    update_clients_if_needed();

    let mut client_opt = None;
    CLIENT_POOL.with(|pool| {
        if let Some(local_clients) = &*pool.borrow() {
            if let Some(client) = local_clients.streaming_clients.get(service_name) {
                client_opt = Some(client.clone());
            }
        }
    });
    client_opt
}

fn build_mtls(mtls: &gate::cfg::mtls::MtlsConfig) -> ClientConfig {
    // read ca cert file
    let mut ca_cert_file = std::io::BufReader::new(
        std::fs::File::open(&mtls.ca_cert_path).expect("Unable to open CA cert file"),
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
        std::fs::File::open(&mtls.client_cert_path).expect("Unable to open client cert file"),
    );
    let client_certs = certs(&mut client_cert_file)
        .collect::<Result<Vec<_>, _>>()
        .expect("Unable to read client certs");

    // read client key file
    let mut client_key_file = std::io::BufReader::new(
        std::fs::File::open(&mtls.client_key_path).expect("Unable to open client key file"),
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
