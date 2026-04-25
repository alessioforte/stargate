use super::{SUPER_ADMIN, extract_json, extract_query};
use crate::err::{ErrorResponse, HttpError};
use crate::require_grants;
use axum::Json;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use gate::cfg::{RuntimeConfig, v2alpha1::Config};
use http::header::CONTENT_TYPE;
use serde::Deserialize;
use std::env;
use std::fs;

#[derive(Deserialize, Debug, utoipa::IntoParams)]
struct ConfigFormatQuery {
    format: Option<String>,
}

const CONFIG_GRANT: &str = "configurations";

#[utoipa::path(
    get,
    path = "/admin/configurations",
    tags = ["Admin", "Configurations"],
    responses(
        (status = 200, description = "OK"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Config file not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_configurations(req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, CONFIG_GRANT);

    let query: ConfigFormatQuery = extract_query(&req)?;
    let format = query.format.unwrap_or_else(|| "json".to_string());

    let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
    let config_filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    let path = format!("{}/{}", config_path, config_filename);

    let content = fs::read_to_string(&path).map_err(|_| {
        ErrorResponse::from(HttpError::NotFound("Config file not found".to_string()))
    })?;

    match format.as_str() {
        "yaml" => Ok(http::Response::builder()
            .status(200)
            .header(CONTENT_TYPE, "application/yaml")
            .body(axum::body::Body::from(content))
            .unwrap()
            .into_response()),
        _ => {
            let config = RuntimeConfig::from_yaml_str(&content).map_err(|e| {
                ErrorResponse::from(HttpError::InternalServerError(format!(
                    "Failed to parse config file: {}",
                    e
                )))
            })?;
            Ok(Json(config.raw).into_response())
        }
    }
}

#[utoipa::path(
    put,
    path = "/admin/configurations",
    tags = ["Admin", "Configurations"],
    responses(
        (status = 200, description = "OK"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_configurations(req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, CONFIG_GRANT);

    let config: Config = extract_json(req).await?;
    RuntimeConfig::from_raw(config.clone()).map_err(|e| {
        ErrorResponse::from(HttpError::BadRequest(format!(
            "Invalid gateway config: {}",
            e
        )))
    })?;

    let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
    let config_filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    config.to_file(&format!("{}/{}", config_path, config_filename));

    Ok(http::Response::builder()
        .status(200)
        .body(axum::body::Body::empty())
        .unwrap()
        .into_response())
}
