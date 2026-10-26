use crate::ctx::sign;
use crate::gate::transport::{self, PreparedTransports, TransportPreparationError};
use gate::cfg::{Config, RuntimeConfig};
use std::{env, path::Path};

#[derive(Debug, thiserror::Error)]
pub(crate) enum GatewayPreparationError {
    #[error(transparent)]
    Config(#[from] gate::cfg::ConfigLoadError),
    #[error(transparent)]
    InternalContext(#[from] sign::InternalContextError),
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
    pub(super) fn reload_outcome(&self) -> &'static str {
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
    pub(super) transports: PreparedTransports,
}

pub(crate) fn prepare_config(raw: Config) -> Result<PreparedConfig, GatewayPreparationError> {
    prepare_runtime_config(RuntimeConfig::from_raw(raw)?)
}

pub(super) fn prepare_runtime_config(
    config: RuntimeConfig,
) -> Result<PreparedConfig, GatewayPreparationError> {
    sign::preflight_config(&config)?;
    let transports = transport::prepare(&config)?;
    Ok(PreparedConfig { config, transports })
}

pub(in crate::gate) fn get_config_path() -> std::io::Result<String> {
    let dir = crate::conf::config_dir();
    if !Path::new(&dir).exists() {
        std::fs::create_dir_all(&dir)?;
    }
    let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    Ok(format!("{}/{}", dir, filename))
}

pub(in crate::gate) fn load_config_from_path(
    path: &str,
) -> Result<PreparedConfig, GatewayPreparationError> {
    prepare_runtime_config(RuntimeConfig::from_file(path)?)
}
