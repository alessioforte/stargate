use crate::ctx::sign;
use crate::data::db;
use crate::err::{ErrorCode, ErrorResponse};
use crate::gate::transport::{self, PreparedTransports, TransportPreparationError};
use ::db::ent::{
    ConfigDocument, ConfigKind, ConfigSaveOutcome, ConfigSource, TrustedAuditBoundary,
    TrustedAuditContext, TrustedBackgroundActor,
};
use gate::cfg::{Config, RuntimeConfig};
use std::{env, sync::Arc};

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
    #[error("unable to serialize gateway configuration: {0}")]
    Serialization(String),
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
            Self::Config(_)
            | Self::Core(_)
            | Self::Serialization(_)
            | Self::VersionExhausted
            | Self::BudgetChange => "error",
            Self::InternalContext(_) => "internal_context_error",
            Self::Transport(_) => "transport_error",
        }
    }
}

pub(crate) struct PreparedConfig {
    pub(crate) config: RuntimeConfig,
    pub(super) config_revision: String,
    pub(super) transports: PreparedTransports,
}

pub(crate) fn prepare_config(raw: Config) -> Result<PreparedConfig, GatewayPreparationError> {
    let yaml = raw
        .to_yaml_string()
        .map_err(|error| GatewayPreparationError::Serialization(error.to_string()))?;
    prepare_runtime_config(
        RuntimeConfig::from_raw(raw)?,
        ::db::ent::config_revision(&yaml),
    )
}

pub(super) fn prepare_runtime_config(
    config: RuntimeConfig,
    config_revision: String,
) -> Result<PreparedConfig, GatewayPreparationError> {
    sign::preflight_config(&config)?;
    let transports = transport::prepare(&config)?;
    Ok(PreparedConfig {
        config,
        config_revision,
        transports,
    })
}

pub(in crate::gate) fn get_config_path() -> String {
    let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    format!("{}/{}", crate::conf::config_dir(), filename)
}

pub(in crate::gate) async fn load_gateway_document() -> anyhow::Result<ConfigDocument> {
    load_gateway_document_from_path(&get_config_path()).await
}

pub(crate) fn validate_gateway_document(content: &str) -> Result<(), gate::cfg::ConfigLoadError> {
    RuntimeConfig::from_yaml_str(content).map(|_| ())
}

async fn load_gateway_document_from_path(path: &str) -> anyhow::Result<ConfigDocument> {
    if let Some(document) = db::get_active_config_document(ConfigKind::Gateway).await? {
        return Ok(document);
    }
    let content = match tokio::fs::read_to_string(path).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("no gateway configuration in the database and no seed file at {path}")
        }
        Err(error) => return Err(error.into()),
    };
    validate_gateway_document(&content)?;
    let context = TrustedAuditContext::background(
        TrustedAuditBoundary::control_plane(),
        TrustedBackgroundActor::system(None),
    );
    match db::save_config_document(
        ConfigKind::Gateway,
        content,
        None,
        ConfigSource::Seed,
        context,
    )
    .await?
    {
        ConfigSaveOutcome::Saved(document) | ConfigSaveOutcome::Unchanged(document) => Ok(document),
        ConfigSaveOutcome::Conflict { .. } => db::get_active_config_document(ConfigKind::Gateway)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("gateway configuration disappeared after concurrent seeding")
            }),
    }
}

pub(in crate::gate) fn prepare_gateway_config(
    yaml: &str,
) -> Result<PreparedConfig, GatewayPreparationError> {
    prepare_runtime_config(
        RuntimeConfig::from_yaml_str(yaml)?,
        ::db::ent::config_revision(yaml),
    )
}

#[derive(serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConfigActivation {
    Applied,
    RestartRequired,
}

pub(crate) async fn save_gateway_config(
    gate: Arc<super::Gate>,
    config: Config,
    revision: String,
    context: TrustedAuditContext,
) -> Result<(ConfigDocument, ConfigActivation), ErrorResponse> {
    let runtime = gate.clone();
    let (yaml, activation) = tokio::task::spawn_blocking(move || {
        let prepared = prepare_config(config).map_err(invalid_config)?;
        let yaml = prepared
            .config
            .raw
            .to_yaml_string()
            .map_err(ErrorResponse::internal)?;
        let activation = match runtime.prepare_activation(prepared) {
            Ok(activation) => Some(activation),
            Err(GatewayPreparationError::BudgetChange) => None,
            Err(error) => return Err(invalid_config(error)),
        };
        Ok::<_, ErrorResponse>((yaml, activation))
    })
    .await
    .map_err(ErrorResponse::internal)??;
    let document = match db::save_config_document(
        ConfigKind::Gateway,
        yaml,
        Some(revision),
        ConfigSource::AdminApi,
        context,
    )
    .await
    .map_err(ErrorResponse::internal)?
    {
        ConfigSaveOutcome::Conflict { .. } => {
            return Err(ErrorResponse::new(ErrorCode::ConfigurationRevisionConflict));
        }
        ConfigSaveOutcome::Saved(document) | ConfigSaveOutcome::Unchanged(document) => document,
    };
    let outcome = match activation {
        Some(activation) => {
            gate.publish(activation).map_err(invalid_config)?;
            crate::obs::telemetry::record_config_reload("gateway_config", "success");
            ConfigActivation::Applied
        }
        None => ConfigActivation::RestartRequired,
    };
    Ok((document, outcome))
}

fn invalid_config(error: GatewayPreparationError) -> ErrorResponse {
    crate::obs::telemetry::record_config_reload("gateway_config", error.reload_outcome());
    ErrorResponse::new(ErrorCode::ConfigurationInvalid).with_message(error.to_string())
}

#[cfg(all(test, feature = "sqlite"))]
mod tests;
