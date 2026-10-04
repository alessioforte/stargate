mod transport;

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(all(test, feature = "memory"))]
#[path = "gate/reload_tests.rs"]
mod reload_tests;

pub use transport::HyperClient;

use crate::etc::{internal_context, store::use_store, telemetry};
use gate::{
    Gate,
    cfg::{Config, RuntimeConfig},
};
use notify::{EventKind, RecursiveMode, Watcher, event::ModifyKind};
use std::{
    env,
    path::Path,
    sync::{
        Arc, OnceLock, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};
use tokio::runtime::Handle;
use tracing::{error, info};

use transport::{PreparedTransports, TransportPreparationError};

#[derive(Debug, thiserror::Error)]
pub(crate) enum GatewayPreparationError {
    #[error(transparent)]
    Config(#[from] gate::cfg::ConfigLoadError),
    #[error(transparent)]
    InternalContext(#[from] internal_context::InternalContextError),
    #[error(transparent)]
    Transport(#[from] TransportPreparationError),
}

impl GatewayPreparationError {
    fn reload_outcome(&self) -> &'static str {
        match self {
            Self::Config(_) => "error",
            Self::InternalContext(_) => "internal_context_error",
            Self::Transport(_) => "transport_error",
        }
    }
}

pub(crate) struct PreparedConfig {
    pub(crate) config: RuntimeConfig,
    transports: PreparedTransports,
}

pub(crate) fn prepare_config(raw: Config) -> Result<PreparedConfig, GatewayPreparationError> {
    prepare_runtime_config(RuntimeConfig::from_raw(raw)?)
}

fn prepare_runtime_config(
    config: RuntimeConfig,
) -> Result<PreparedConfig, GatewayPreparationError> {
    internal_context::preflight_config(&config)?;
    let transports = transport::prepare(&config)?;
    Ok(PreparedConfig { config, transports })
}

fn get_config_dir() -> String {
    env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string())
}

fn get_config_path() -> std::io::Result<String> {
    let dir = get_config_dir();
    if !Path::new(&dir).exists() {
        std::fs::create_dir_all(&dir)?;
    }
    let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    Ok(format!("{}/{}", dir, filename))
}

pub fn get_policies_path() -> String {
    let dir = get_config_dir();
    if !Path::new(&dir).exists() {
        std::fs::create_dir(&dir).expect("Unable to create config directory");
    }
    let filename = "policies";
    let path = format!("{}/{}", dir, filename);
    if !Path::new(&path).exists() {
        std::fs::write(&path, "").expect("Unable to create policies file");
    }
    path
}

fn load_config_from_path(path: &str) -> Result<PreparedConfig, GatewayPreparationError> {
    prepare_runtime_config(RuntimeConfig::from_file(path)?)
}

pub fn init() -> anyhow::Result<Arc<Gate>> {
    let config_file_path = get_config_path()?;
    let prepared = load_config_from_path(&config_file_path)?;
    let store = use_store();
    let policies_path = get_policies_path();
    let policies = crate::act::access_control_rules::load_policy_snapshot(&policies_path)
        .map_err(|error| anyhow::anyhow!("Unable to load access-control policies: {error}"))?;

    let gate = Gate::new(Arc::new(store.clone())).build(&prepared.config, policies);
    update_cached_config(config_cache(), prepared);
    watch_config_file(&config_file_path, &gate);
    watch_policies_file(&policies_path, &gate);
    Ok(Arc::new(gate))
}

static CONFIG_VERSION: AtomicU64 = AtomicU64::new(0);
static CONFIG_CACHE: OnceLock<RwLock<Option<CachedConfig>>> = OnceLock::new();

struct CachedConfig {
    version: u64,
    prepared: PreparedConfig,
}

fn config_cache() -> &'static RwLock<Option<CachedConfig>> {
    CONFIG_CACHE.get_or_init(|| RwLock::new(None))
}

fn update_cached_config(cache: &RwLock<Option<CachedConfig>>, prepared: PreparedConfig) -> u64 {
    let mut cache = cache.write().expect("Config cache lock poisoned");
    let version = cache.as_ref().map_or(0, |active| active.version + 1);
    *cache = Some(CachedConfig { version, prepared });
    version
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn set_config_for_test(config: RuntimeConfig) {
    let prepared = prepare_runtime_config(config).expect("Invalid test gateway configuration");
    let version = update_cached_config(config_cache(), prepared);
    CONFIG_VERSION.store(version, Ordering::SeqCst);
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
                        match reload_gateway_config_from_path(&mut gate, &file_path, config_cache()).await {
                            Ok(config_version) => {
                                last_content = current_content;
                                info!(config_version, "Configuration file changed, reloaded");
                                telemetry::record_config_reload("gateway_config", "success");
                            }
                            Err(error) => {
                                telemetry::record_config_reload("gateway_config", error.reload_outcome());
                                error!(%error, "Configuration file changed but did not validate; keeping previous config");
                            }
                        }
                    }
                    Err(e) => {
                        telemetry::record_config_reload("gateway_config", "error");
                        error!("Watch error: {:?}", e);
                    }
                }
            }
        });
    });
}

async fn reload_gateway_config_from_path(
    gate: &mut Gate,
    path: &str,
    cache: &RwLock<Option<CachedConfig>>,
) -> Result<u64, GatewayPreparationError> {
    let prepared = load_config_from_path(path)?;
    gate.update_config(&prepared.config).await;
    let version = update_cached_config(cache, prepared);
    CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
    Ok(version)
}

fn watch_policies_file(file_path: &str, gate: &Gate) {
    let file_path = file_path.to_string();
    let gate = gate.clone();
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

                        info!("Policies file changed, reloading...");
                        if let Err(error) = reload_policy_engine_from_path(&gate, &file_path) {
                            telemetry::record_config_reload("policies", "error");
                            error!(%error, "Policy file changed but did not validate; keeping previous policies");
                        }
                    }
                    Err(e) => {
                        telemetry::record_config_reload("policies", "error");
                        error!("Watch error: {:?}", e);
                    }
                }
            }
        });
    });
}

pub fn get_config_version() -> u64 {
    CONFIG_VERSION.load(Ordering::SeqCst)
}

pub fn reload_policy_engine(gate: &Gate) -> Result<(), crate::err::ErrorResponse> {
    let policies_path = get_policies_path();
    reload_policy_engine_from_path(gate, &policies_path).map(|_| ())
}

fn reload_policy_engine_from_path(
    gate: &Gate,
    policies_path: &str,
) -> Result<bool, crate::err::ErrorResponse> {
    let policies = crate::act::access_control_rules::load_policy_snapshot(policies_path)?;
    if gate.policy_snapshot.load().revision == policies.revision {
        return Ok(false);
    }

    gate.update_policy_snapshot(policies);
    CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
    telemetry::record_config_reload("policies", "success");
    Ok(true)
}

pub fn get_client(service_name: &str) -> Option<HyperClient> {
    config_cache()
        .read()
        .expect("Config cache lock poisoned")
        .as_ref()?
        .prepared
        .transports
        .get(service_name)
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::{
        load_config_from_path, reload_gateway_config_from_path, reload_policy_engine_from_path,
        update_cached_config,
    };
    use gate::{
        Gate, PolicySnapshot,
        cfg::{RuntimeConfig, SCHEMA},
    };
    use std::{fs, sync::Arc};

    fn policy_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "stargate-policy-reload-{}-{}",
            std::process::id(),
            ulid::Ulid::generate()
        ))
    }

    #[tokio::test]
    async fn unsupported_schema_reload_keeps_the_active_runtime() {
        let path = policy_path();
        let path_str = path.to_str().unwrap();
        fs::write(
            &path,
            r#"
schema: stargate/v1
http:
  services:
    healthy:
      kind: direct_response
      status: 200
      body:
        text: ready
  routers:
    healthy:
      match:
        path:
          prefix: /
      service: healthy
"#,
        )
        .unwrap();
        let prepared = load_config_from_path(path_str).unwrap();
        let config = prepared.config.clone();
        let cache = std::sync::RwLock::new(None);
        update_cached_config(&cache, prepared);
        let mut gate =
            Gate::new(Arc::new(lim::State::new())).build(&config, PolicySnapshot::default());
        let graph = gate.http_graph.load_full();
        let balancers = gate.http_balancers.load_full();
        let limiter = gate.limiter.load_full();
        assert_eq!(graph.routers[0].service, "healthy");

        for yaml in [
            "schema: stargate/v2alpha1\nhttp: {}\n",
            "schema: stargate/v2\nhttp: {}\n",
            "http: {}\n",
        ] {
            fs::write(&path, yaml).unwrap();
            assert!(load_config_from_path(path_str).is_err());
            let error = reload_gateway_config_from_path(&mut gate, path_str, &cache)
                .await
                .unwrap_err();
            assert!(error.to_string().contains(SCHEMA));
            assert!(Arc::ptr_eq(&gate.http_graph.load_full(), &graph));
            assert!(Arc::ptr_eq(&gate.http_balancers.load_full(), &balancers));
            assert!(Arc::ptr_eq(&gate.limiter.load_full(), &limiter));
        }
        assert_eq!(
            RuntimeConfig::from_raw(config.raw).unwrap().compiled.schema,
            SCHEMA
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn invalid_policy_reload_keeps_the_active_snapshot() {
        let path = policy_path();
        let path_str = path.to_str().unwrap();
        let gate = Gate::new(Arc::new(lim::State::new()));

        fs::write(&path, "ALLOW user FOR \"reports:READ\";").unwrap();
        assert!(reload_policy_engine_from_path(&gate, path_str).unwrap());
        let initial_revision = gate.policy_snapshot.load().revision.clone();
        assert!(!reload_policy_engine_from_path(&gate, path_str).unwrap());

        fs::write(&path, "ALLOW user WHEN invalid;").unwrap();
        assert!(reload_policy_engine_from_path(&gate, path_str).is_err());
        assert_eq!(gate.policy_snapshot.load().revision, initial_revision);

        fs::write(&path, "ALLOW user FOR \"dashboard\";").unwrap();
        assert!(reload_policy_engine_from_path(&gate, path_str).unwrap());
        assert_ne!(gate.policy_snapshot.load().revision, initial_revision);

        fs::remove_file(path).unwrap();
    }
}
