pub mod resources;
mod transport;

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(all(test, feature = "memory"))]
#[path = "gate/reload_tests.rs"]
mod reload_tests;

#[cfg(test)]
pub(crate) use transport::DeadlineConnector;
pub use transport::{HyperClient, PreparedTransport, connection_timed_out};

use crate::etc::{internal_context, store::use_store, telemetry};
use arc_swap::ArcSwap;
use gate::{
    PolicySnapshot, RuntimeBuilder,
    cfg::{Config, RuntimeConfig},
};
use notify::{EventKind, RecursiveMode, Watcher, event::ModifyKind};
use std::{
    env,
    path::Path,
    sync::{Arc, Mutex},
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
    #[error(transparent)]
    Core(#[from] gate::graph::CompileError),
    #[error("gateway configuration version exhausted")]
    VersionExhausted,
    #[error(
        "process gateway budgets cannot change on reload; restart the process to apply runtime capacities"
    )]
    BudgetChange,
}

impl GatewayPreparationError {
    fn reload_outcome(&self) -> &'static str {
        match self {
            Self::Config(_) | Self::Core(_) | Self::VersionExhausted | Self::BudgetChange => {
                "error"
            }
            Self::InternalContext(_) => "internal_context_error",
            Self::Transport(_) => "transport_error",
        }
    }
}

pub(crate) struct PreparedConfig {
    pub(crate) config: RuntimeConfig,
    transports: PreparedTransports,
}

pub struct RuntimeSnapshot {
    pub core: gate::Runtime,
    pub version: u64,
    pub ingress: gate::cfg::CompiledIngress,
    pub settings: gate::cfg::CompiledRuntimeSettings,
    pub resources: Arc<resources::ProcessResources>,
    transports: PreparedTransports,
}

impl RuntimeSnapshot {
    pub fn client(&self, service: &str) -> Option<HyperClient> {
        self.transport(service)
            .map(|transport| transport.http.clone())
    }

    pub fn transport(&self, service: &str) -> Option<&PreparedTransport> {
        self.transports.get(service)
    }
}

pub fn retain_runtime(body: axum::body::Body, runtime: Arc<RuntimeSnapshot>) -> axum::body::Body {
    axum::body::Body::new(RuntimeBody {
        body,
        _runtime: runtime,
    })
}

struct RuntimeBody {
    body: axum::body::Body,
    _runtime: Arc<RuntimeSnapshot>,
}

impl hyper::body::Body for RuntimeBody {
    type Data = hyper::body::Bytes;
    type Error = axum::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        std::pin::Pin::new(&mut self.get_mut().body).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.body.size_hint()
    }
}

pub struct Gate {
    builder: RuntimeBuilder,
    runtime: ArcSwap<RuntimeSnapshot>,
    activation: Mutex<()>,
    pub policy_snapshot: ArcSwap<PolicySnapshot>,
    pub resources: Arc<resources::ProcessResources>,
}

impl Gate {
    pub(crate) fn new(
        store: Arc<lim::State>,
        prepared: PreparedConfig,
        policies: PolicySnapshot,
    ) -> Result<Self, GatewayPreparationError> {
        let builder = RuntimeBuilder::new(store);
        let core = builder.prepare(&prepared.config)?;
        let settings = prepared.config.compiled.runtime.clone();
        let resources = resources::ProcessResources::new(settings.budgets);
        let runtime = Arc::new(RuntimeSnapshot {
            core,
            ingress: prepared.config.compiled.ingress.clone(),
            settings,
            resources: resources.clone(),
            transports: prepared.transports,
            version: 0,
        });
        runtime.core.start_probes();
        Ok(Self {
            builder,
            resources,
            runtime: ArcSwap::from(runtime),
            activation: Mutex::new(()),
            policy_snapshot: ArcSwap::from_pointee(policies),
        })
    }

    pub fn snapshot(&self) -> Arc<RuntimeSnapshot> {
        self.runtime.load_full()
    }

    pub(crate) fn activate(
        &self,
        prepared: PreparedConfig,
    ) -> Result<u64, GatewayPreparationError> {
        if prepared.config.compiled.runtime.budgets != self.resources.budgets {
            return Err(GatewayPreparationError::BudgetChange);
        }
        let core = self.builder.prepare(&prepared.config)?;
        // Serialize only activation: file reads, TLS and core construction all
        // finish before touching the active generation or its probes.
        let _activation = self.activation.lock().expect("Activation lock poisoned");
        let version = self
            .runtime
            .load()
            .version
            .checked_add(1)
            .ok_or(GatewayPreparationError::VersionExhausted)?;
        let runtime = Arc::new(RuntimeSnapshot {
            core,
            ingress: prepared.config.compiled.ingress.clone(),
            settings: prepared.config.compiled.runtime.clone(),
            resources: self.resources.clone(),
            transports: prepared.transports,
            version,
        });
        let previous = self.runtime.swap(runtime.clone());
        runtime.core.start_probes();
        previous.core.stop_probes();
        Ok(version)
    }
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

    let gate = Arc::new(Gate::new(Arc::new(store.clone()), prepared, policies)?);
    watch_config_file(&config_file_path, &gate);
    watch_policies_file(&policies_path, &gate);
    Ok(gate)
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

fn watch_config_file(file_path: &str, gate: &Arc<Gate>) {
    let file_path = file_path.to_string();
    let gate = gate.clone();
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
                        match reload_gateway_config_from_path(&gate, &file_path) {
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

fn reload_gateway_config_from_path(
    gate: &Gate,
    path: &str,
) -> Result<u64, GatewayPreparationError> {
    let prepared = load_config_from_path(path)?;
    gate.activate(prepared)
}

fn watch_policies_file(file_path: &str, gate: &Arc<Gate>) {
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

    gate.policy_snapshot.store(Arc::new(policies));
    telemetry::record_config_reload("policies", "success");
    Ok(true)
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::{
        Gate, load_config_from_path, reload_gateway_config_from_path,
        reload_policy_engine_from_path,
    };
    use gate::{
        PolicySnapshot,
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
ingress:
  limit: ingress
  timeout: 250ms
limits:
  default:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 1s
  ingress:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 100ms
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
        let gate = Gate::new(
            Arc::new(lim::State::new()),
            prepared,
            PolicySnapshot::default(),
        )
        .unwrap();
        let runtime = gate.snapshot();
        assert_eq!(runtime.core.graph.routers[0].service, "healthy");

        for yaml in [
            "schema: stargate/v2alpha1\nhttp: {}\n",
            "schema: stargate/v2\nhttp: {}\n",
            "http: {}\n",
        ] {
            fs::write(&path, yaml).unwrap();
            assert!(load_config_from_path(path_str).is_err());
            let error = reload_gateway_config_from_path(&gate, path_str).unwrap_err();
            assert!(error.to_string().contains(SCHEMA));
            assert!(Arc::ptr_eq(&gate.snapshot(), &runtime));
            assert_eq!(gate.snapshot().version, 0);
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
        let gate = super::test_support::gate(gate::cfg::Config::default());
        let runtime = gate.snapshot();

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

        assert!(Arc::ptr_eq(&runtime, &gate.snapshot()));
        assert_eq!(gate.snapshot().version, 0);
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn response_body_retains_its_generation_until_consumed_or_dropped() {
        use axum::body::Body;
        use http::Request;
        use http_body_util::BodyExt;

        let config = RuntimeConfig::from_yaml_str(
            r#"
schema: stargate/v1
ingress:
  limit: ingress
  timeout: 250ms
limits:
  default:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 1s
  ingress:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 100ms
http:
  services:
    local:
      kind: direct_response
      status: 200
      body:
        text: ready
  routers:
    local:
      match:
        path:
          prefix: /
      service: local
"#,
        )
        .unwrap()
        .raw;
        for consume in [true, false] {
            let gate = Arc::new(super::test_support::gate(config.clone()));
            let mut req = Request::builder().uri("/").body(Body::empty()).unwrap();
            req.extensions_mut().insert(gate.clone());
            let body = crate::api::gateway::service(req).await.unwrap().into_body();
            let retained = Arc::downgrade(&gate.snapshot());
            gate.activate(super::prepare_config(gate::cfg::Config::default()).unwrap())
                .unwrap();
            assert!(retained.upgrade().is_some());
            if consume {
                assert_eq!(body.collect().await.unwrap().to_bytes(), "ready");
            } else {
                drop(body);
            }
            assert!(retained.upgrade().is_none());
        }
    }
}
