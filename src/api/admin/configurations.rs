use super::{
    authorization::{self, Permission},
    extract_json, extract_query, take_admin_audit_context,
};
use crate::data::db;
use crate::err::{ErrorCode, ErrorResponse};
use crate::gate::{ConfigActivation, Gate, save_gateway_config};
use ::db::ent::{ConfigDocument, ConfigKind};
use axum::{
    Json,
    extract::Request,
    response::{IntoResponse, Response},
};
use gate::cfg::{Config, RuntimeConfig};
use http::header::{CONTENT_TYPE, ETAG};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize, Debug, utoipa::IntoParams)]
struct ConfigFormatQuery {
    format: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationDocument {
    revision: String,
    version: i64,
    updated_at: String,
    updated_by: Option<String>,
    #[schema(value_type = Object)]
    config: Config,
}

impl ConfigurationDocument {
    fn from_document(document: ConfigDocument) -> Result<Self, ErrorResponse> {
        let config = RuntimeConfig::from_yaml_str(&document.content)
            .map_err(ErrorResponse::internal)?
            .raw;
        Ok(Self {
            revision: document.revision,
            version: document.version,
            updated_at: document.created_at.to_rfc3339(),
            updated_by: document.created_by,
            config,
        })
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateConfigurationRequest {
    revision: String,
    #[schema(value_type = Object)]
    config: Config,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct UpdateConfigurationResponse {
    #[serde(flatten)]
    document: ConfigurationDocument,
    activation: ConfigActivation,
}

#[utoipa::path(
    get,
    path = "/admin/configurations",
    tags = ["Admin", "Configurations"],
    params(ConfigFormatQuery),
    responses(
        (status = 200, description = "Gateway configuration document", body = ConfigurationDocument),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Gateway configuration not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_configurations(req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::ConfigurationsRead)?;
    let query: ConfigFormatQuery = extract_query(&req)?;
    let document = db::get_active_config_document(ConfigKind::Gateway)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::ConfigurationNotFound))?;
    let etag = format!("\"{}\"", document.revision);
    let mut response = if query.format.as_deref() == Some("yaml") {
        ([(CONTENT_TYPE, "application/yaml")], document.content).into_response()
    } else {
        Json(ConfigurationDocument::from_document(document)?).into_response()
    };
    response
        .headers_mut()
        .insert(ETAG, etag.parse().map_err(ErrorResponse::internal)?);
    Ok(response)
}

#[utoipa::path(
    put,
    path = "/admin/configurations",
    tags = ["Admin", "Configurations"],
    request_body = UpdateConfigurationRequest,
    responses(
        (status = 200, description = "Configuration saved", body = UpdateConfigurationResponse),
        (status = 400, description = "Invalid configuration", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 409, description = "Configuration revision conflict", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_configurations(mut req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::ConfigurationsUpdate)?;
    let context = take_admin_audit_context(req.extensions_mut())?;
    let gate = req
        .extensions()
        .get::<Arc<Gate>>()
        .cloned()
        .ok_or_else(|| ErrorResponse::internal("Gateway runtime extension is missing"))?;
    let payload: UpdateConfigurationRequest = extract_json(req).await?;
    let (document, activation) =
        save_gateway_config(gate, payload.config, payload.revision, context).await?;
    Ok(Json(UpdateConfigurationResponse {
        document: ConfigurationDocument::from_document(document)?,
        activation,
    })
    .into_response())
}

#[cfg(all(test, feature = "sqlite"))]
mod tests;
