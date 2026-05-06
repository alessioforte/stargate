use crate::etc::store::use_store;
use gate::{
    Gate,
    cfg::{RuntimeConfig, Service},
};
use notify::{EventKind, RecursiveMode, Watcher, event::ModifyKind};
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

use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use rustls::{ClientConfig, RootCertStore};
use rustls_pemfile::{certs, pkcs8_private_keys};

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

fn load_config() -> RuntimeConfig {
    let config_file_path = get_config_path();
    RuntimeConfig::from_file(&config_file_path).expect("Unable to load gateway config")
}

pub fn init() -> std::sync::Arc<Gate> {
    let store = use_store();
    let config = get_config();
    let config_file_path = get_config_path();
    let policies_path = get_policies_path();

    let gate = Gate::new(Arc::new(store.clone())).build(config.as_ref(), &policies_path);
    watch_config_file(&config_file_path, &gate);
    watch_policies_file(&policies_path, &gate);
    Arc::new(gate)
}

static CONFIG_VERSION: AtomicU64 = AtomicU64::new(0);
static CONFIG_CACHE: OnceLock<RwLock<CachedConfig>> = OnceLock::new();

#[derive(Clone)]
struct CachedConfig {
    version: u64,
    config: Arc<RuntimeConfig>,
}

fn config_cache() -> &'static RwLock<CachedConfig> {
    CONFIG_CACHE.get_or_init(|| {
        RwLock::new(CachedConfig {
            version: 0,
            config: Arc::new(load_config()),
        })
    })
}

fn get_config() -> Arc<RuntimeConfig> {
    let cache = config_cache().read().expect("Config cache lock poisoned");
    cache.config.clone()
}

fn get_config_snapshot() -> CachedConfig {
    config_cache()
        .read()
        .expect("Config cache lock poisoned")
        .clone()
}

fn update_cached_config(config: RuntimeConfig) -> u64 {
    let mut cache = config_cache().write().expect("Config cache lock poisoned");
    cache.version += 1;
    cache.config = Arc::new(config);
    cache.version
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn set_config_for_test(config: RuntimeConfig) {
    let version = update_cached_config(config);
    CONFIG_VERSION.store(version, Ordering::SeqCst);
    let mut pool = hyper_client_pool()
        .write()
        .expect("Hyper client pool lock poisoned");
    *pool = None;
}

fn file_content(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

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

                        let config = match RuntimeConfig::from_file(&file_path) {
                            Ok(config) => config,
                            Err(error) => {
                                error!(%error, "Configuration file changed but did not validate; keeping previous config");
                                continue;
                            }
                        };
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

type HyperConnector = hyper_rustls::HttpsConnector<HttpConnector>;
pub type HyperClient = Client<HyperConnector, axum::body::Body>;

#[derive(Clone)]
struct SharedHyperClients {
    version: u64,
    clients: HashMap<String, HyperClient>,
}

static HYPER_CLIENT_POOL: OnceLock<RwLock<Option<SharedHyperClients>>> = OnceLock::new();

fn hyper_client_pool() -> &'static RwLock<Option<SharedHyperClients>> {
    HYPER_CLIENT_POOL.get_or_init(|| RwLock::new(None))
}

struct BuiltHyperClients {
    clients: HashMap<String, HyperClient>,
}

fn build_hyper_client(timeout: Duration, tls_config: Option<&ClientConfig>) -> HyperClient {
    crate::etc::tls::install_crypto_provider();

    let mut connector = HttpConnector::new();
    connector.enforce_http(false);
    connector.set_connect_timeout(Some(timeout));

    let https = match tls_config {
        Some(tls_config) => HttpsConnectorBuilder::new()
            .with_tls_config(tls_config.clone())
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .wrap_connector(connector),
        None => HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .wrap_connector(connector),
    };

    Client::builder(TokioExecutor::new()).build(https)
}

fn build_hyper_clients(config: &RuntimeConfig) -> BuiltHyperClients {
    let mut clients = HashMap::new();

    let tls_config = config.mtls().map(build_mtls);

    for (name, service) in &config.raw.http.services {
        let Service::LoadBalancer { upstream } = service else {
            continue;
        };
        let Some(upstream) = config.raw.http.upstreams.get(upstream) else {
            continue;
        };

        let timeout = upstream
            .transport
            .as_ref()
            .and_then(|transport| transport.connect_timeout.as_deref())
            .and_then(|duration| tools::parse_duration(duration).ok())
            .and_then(|duration| duration.to_std().ok())
            .unwrap_or_else(|| Duration::from_secs(30));
        let use_tls = tls_config.is_some()
            && upstream
                .targets
                .iter()
                .any(|target| target.url.starts_with("https://"));
        let tls = use_tls.then_some(tls_config.as_ref()).flatten();

        let client = build_hyper_client(timeout, tls);
        clients.insert(name.clone(), client);
    }

    BuiltHyperClients { clients }
}

fn update_clients_if_needed() {
    let config_snapshot = get_config_snapshot();
    let mut pool = hyper_client_pool()
        .write()
        .expect("Hyper client pool lock poisoned");

    let needs_update = match &*pool {
        Some(shared_clients) => shared_clients.version != config_snapshot.version,
        None => true,
    };

    if needs_update {
        let built = build_hyper_clients(config_snapshot.config.as_ref());
        *pool = Some(SharedHyperClients {
            version: config_snapshot.version,
            clients: built.clients,
        });
    }
}

pub fn get_client(service_name: &str) -> Option<HyperClient> {
    update_clients_if_needed();
    hyper_client_pool()
        .read()
        .expect("Hyper client pool lock poisoned")
        .as_ref()
        .and_then(|pool| pool.clients.get(service_name))
        .cloned()
}

fn build_mtls(mtls: &gate::cfg::MtlsConfig) -> ClientConfig {
    let mut ca_cert_file = std::io::BufReader::new(
        std::fs::File::open(&mtls.ca_cert_path).expect("Unable to open CA cert file"),
    );

    let ca_certs = certs(&mut ca_cert_file)
        .collect::<Result<Vec<_>, _>>()
        .expect("Unable to read CA certs");

    let mut root_store = RootCertStore::empty();
    for cert in ca_certs {
        root_store
            .add(cert)
            .expect("Unable to add CA cert to root store");
    }

    let mut client_cert_file = std::io::BufReader::new(
        std::fs::File::open(&mtls.client_cert_path).expect("Unable to open client cert file"),
    );
    let client_certs = certs(&mut client_cert_file)
        .collect::<Result<Vec<_>, _>>()
        .expect("Unable to read client certs");

    let mut client_key_file = std::io::BufReader::new(
        std::fs::File::open(&mtls.client_key_path).expect("Unable to open client key file"),
    );
    let mut client_keys = pkcs8_private_keys(&mut client_key_file)
        .collect::<Result<Vec<_>, _>>()
        .expect("Unable to read client private keys");

    if client_keys.is_empty() {
        panic!("No client private keys found");
    }

    ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_client_auth_cert(client_certs, client_keys.remove(0).into())
        .expect("Unable to create MTLS client config")
}
