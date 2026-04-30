mod endpoint;
pub mod graph;
mod limit;
mod load_balancer;
mod mtls;
mod v2alpha1;

pub use limit::{Limit, LimitSpec};
pub use load_balancer::LoadBalancer;
pub use mtls::MtlsConfig;
pub use v2alpha1::{AuthStrategy, Config, EnvProfile, Service, UpstreamProtocol};

use graph::CompiledConfig;
use serde_yaml_bw::Value;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::Path;
use tracing::info;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub raw: v2alpha1::Config,
    pub compiled: CompiledConfig,
}

#[derive(Debug)]
pub enum ConfigLoadError {
    Io {
        path: String,
        source: std::io::Error,
    },
    Parse(serde_yaml_bw::Error),
    Compile(graph::CompileError),
    MissingV2Schema,
}

impl Display for ConfigLoadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigLoadError::Io { path, source } => {
                write!(f, "unable to read config '{}': {}", path, source)
            }
            ConfigLoadError::Parse(source) => write!(f, "unable to parse config: {}", source),
            ConfigLoadError::Compile(source) => write!(f, "unable to compile config: {}", source),
            ConfigLoadError::MissingV2Schema => {
                write!(f, "config schema must be 'stargate/v2alpha1'")
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
            ConfigLoadError::MissingV2Schema => None,
        }
    }
}

impl RuntimeConfig {
    pub fn from_file(path: &str) -> Result<Self, ConfigLoadError> {
        if !Path::new(path).exists() {
            info!("Creating v2alpha1 gate configuration yaml file");
            let raw = v2alpha1::Config::default();
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
        ensure_v2alpha1_schema(yaml)?;
        let raw: v2alpha1::Config =
            serde_yaml_bw::from_str(yaml).map_err(ConfigLoadError::Parse)?;
        Self::from_raw(raw)
    }

    pub fn from_raw(raw: v2alpha1::Config) -> Result<Self, ConfigLoadError> {
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

    pub fn raw(&self) -> &v2alpha1::Config {
        &self.raw
    }
}

fn ensure_v2alpha1_schema(yaml: &str) -> Result<(), ConfigLoadError> {
    let value = serde_yaml_bw::from_str::<Value>(yaml).map_err(ConfigLoadError::Parse)?;
    let Some(schema) = value
        .as_mapping()
        .and_then(|mapping| mapping.get(Value::String("schema".to_string(), None)))
        .and_then(Value::as_str)
    else {
        return Err(ConfigLoadError::MissingV2Schema);
    };
    if schema == "stargate/v2alpha1" {
        Ok(())
    } else {
        Err(ConfigLoadError::MissingV2Schema)
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeConfig;

    #[test]
    fn runtime_config_rejects_missing_v2_schema() {
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

        assert!(error.is_err());
    }

    #[test]
    fn runtime_config_loads_v2alpha1_schema() {
        let config = RuntimeConfig::from_yaml_str(
            r#"
schema: stargate/v2alpha1
http:
  upstreams: {}
  services: {}
  middlewares: {}
  policies: {}
  routers: {}
"#,
        )
        .expect("v2 config should load");

        assert_eq!(config.raw.schema, "stargate/v2alpha1");
    }
}
