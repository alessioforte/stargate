mod endpoint;
pub mod graph;
mod ingress;
mod limit;
mod load_balancer;
mod mtls;
mod network;
mod runtime;
mod v1;

pub use ingress::{CompiledIngress, Ingress};
pub use limit::{Limit, LimitSpec};
pub use load_balancer::LoadBalancer;
pub use mtls::MtlsConfig;
pub use runtime::{CompiledRuntimeSettings, ProcessBudgets, ResponseMode, RuntimeSettings};
pub use v1::{
    AuthStrategy, Config, EnvProfile, InternalContext, LimitScope, OnMissingOrg, Service, Upstream,
    UpstreamProtocol,
};

use graph::CompiledConfig;
use serde_json::Value;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::Path;
use tracing::info;

pub const SCHEMA: &str = "stargate/v1";

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub raw: v1::Config,
    pub compiled: CompiledConfig,
}

#[derive(Debug)]
pub enum ConfigLoadError {
    Io {
        path: String,
        source: std::io::Error,
    },
    Parse(serde_saphyr::Error),
    Compile(graph::CompileError),
    InvalidSchema,
}

impl Display for ConfigLoadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigLoadError::Io { path, source } => {
                write!(f, "unable to read config '{}': {}", path, source)
            }
            ConfigLoadError::Parse(source) => write!(f, "unable to parse config: {}", source),
            ConfigLoadError::Compile(source) => write!(f, "unable to compile config: {}", source),
            ConfigLoadError::InvalidSchema => {
                write!(f, "config requires an explicit schema of '{}'", SCHEMA)
            }
        }
    }
}

impl Error for ConfigLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ConfigLoadError::Io { source, .. } => Some(source),
            ConfigLoadError::Parse(source) => Some(source),
            ConfigLoadError::Compile(source) => Some(source),
            ConfigLoadError::InvalidSchema => None,
        }
    }
}

impl RuntimeConfig {
    pub fn from_file(path: &str) -> Result<Self, ConfigLoadError> {
        if !Path::new(path).exists() {
            info!(schema = SCHEMA, "Creating gate configuration yaml file");
            let raw = v1::Config::default();
            raw.to_file(path);
            return Self::from_raw(raw);
        }

        let file = std::fs::read_to_string(path).map_err(|source| ConfigLoadError::Io {
            path: path.to_string(),
            source,
        })?;
        Self::from_yaml_str(&file)
    }

    pub fn from_yaml_str(yaml: &str) -> Result<Self, ConfigLoadError> {
        ensure_schema(yaml)?;
        let raw: v1::Config = serde_saphyr::from_str(yaml).map_err(ConfigLoadError::Parse)?;
        Self::from_raw(raw)
    }

    pub fn from_raw(raw: v1::Config) -> Result<Self, ConfigLoadError> {
        let compiled = raw.compile().map_err(ConfigLoadError::Compile)?;
        Ok(Self { raw, compiled })
    }

    pub fn mtls(&self) -> Option<&mtls::MtlsConfig> {
        self.raw.mtls.as_ref()
    }

    pub fn limits(&self) -> &[limit::Limit] {
        &self.compiled.limits
    }

    pub fn compiled(&self) -> &CompiledConfig {
        &self.compiled
    }

    pub fn raw(&self) -> &v1::Config {
        &self.raw
    }
}

fn ensure_schema(yaml: &str) -> Result<(), ConfigLoadError> {
    let value = serde_saphyr::from_str::<Value>(yaml).map_err(ConfigLoadError::Parse)?;
    let Some(schema) = value
        .as_object()
        .and_then(|mapping| mapping.get("schema"))
        .and_then(Value::as_str)
    else {
        return Err(ConfigLoadError::InvalidSchema);
    };
    if schema == SCHEMA {
        Ok(())
    } else {
        Err(ConfigLoadError::InvalidSchema)
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, ConfigLoadError, RuntimeConfig, SCHEMA};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn config_path() -> std::path::PathBuf {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "stargate-config-{}-{}.yaml",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn runtime_config_rejects_missing_schema() {
        let error = RuntimeConfig::from_yaml_str(
            r#"
limits:
  default:
    strategy: gcra
    params:
      max_burst: 3
      replenish_1_per: 500ms
services: []
"#,
        );

        assert!(matches!(error, Err(ConfigLoadError::InvalidSchema)));
    }

    #[test]
    fn runtime_config_loads_v1_schema() {
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
  upstreams: {}
  services: {}
  middlewares: {}
  policies: {}
  routers: {}
"#,
        )
        .expect("v1 config should load");

        assert_eq!(config.raw.schema, SCHEMA);
        assert_eq!(config.compiled.schema, SCHEMA);
        let yaml = serde_saphyr::to_string(&config.raw).unwrap();
        assert!(yaml.contains("schema: stargate/v1"));
        assert_eq!(
            RuntimeConfig::from_yaml_str(&yaml).unwrap().raw.schema,
            SCHEMA
        );
    }

    #[test]
    fn external_config_loaders_reject_unsupported_schemas() {
        let path = config_path();
        let path_str = path.to_str().unwrap();
        for schema in ["stargate/v2alpha1", "stargate/v2", "other/v1", ""] {
            let yaml = format!("schema: '{schema}'\nhttp: {{}}\n");
            let error = RuntimeConfig::from_yaml_str(&yaml).unwrap_err();
            assert!(matches!(error, ConfigLoadError::InvalidSchema));
            assert!(error.to_string().contains(SCHEMA));

            std::fs::write(&path, &yaml).unwrap();
            assert!(matches!(
                RuntimeConfig::from_file(path_str),
                Err(ConfigLoadError::InvalidSchema)
            ));

            let raw = Config {
                schema: schema.to_string(),
                ..Config::default()
            };
            assert!(raw.compile().unwrap_err().to_string().contains(SCHEMA));
            assert!(matches!(
                RuntimeConfig::from_raw(raw),
                Err(ConfigLoadError::Compile(_))
            ));
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn external_config_loaders_require_an_explicit_schema() {
        let path = config_path();
        let path_str = path.to_str().unwrap();
        for yaml in ["http: {}\n", "schema: null\n", "schema: 1\n"] {
            std::fs::write(&path, yaml).unwrap();
            assert!(matches!(
                RuntimeConfig::from_file(path_str),
                Err(ConfigLoadError::InvalidSchema)
            ));
            if let Ok(raw) = serde_saphyr::from_str::<Config>(yaml) {
                assert!(raw.compile().is_err());
            }
        }
        assert!(serde_saphyr::from_str::<Config>("http: {}\n").is_err());
        assert!(serde_json::from_value::<Config>(serde_json::json!({ "http": {} })).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn generated_default_file_uses_the_supported_schema() {
        let path = config_path();
        let config = RuntimeConfig::from_file(path.to_str().unwrap()).unwrap();
        assert_eq!(config.raw.schema, SCHEMA);
        assert_eq!(config.compiled.schema, SCHEMA);
        let yaml = std::fs::read_to_string(&path).unwrap();
        assert!(yaml.contains("schema: stargate/v1"));
        assert_eq!(
            RuntimeConfig::from_yaml_str(&yaml).unwrap().raw.schema,
            SCHEMA
        );
        std::fs::remove_file(path).unwrap();
    }
}
