use super::{
    authorization::{self, Permission},
    extract_json, extract_query,
};
use crate::err::{ErrorCode, ErrorResponse};
use axum::Json;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use gate::cfg::{Config, RuntimeConfig};
use http::header::CONTENT_TYPE;
use serde::Deserialize;
use std::env;
use std::fs;

#[derive(Deserialize, Debug, utoipa::IntoParams)]
struct ConfigFormatQuery {
    format: Option<String>,
}

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
    authorization::require(&req, Permission::ConfigurationsRead)?;

    let query: ConfigFormatQuery = extract_query(&req)?;
    let format = query.format.unwrap_or_else(|| "json".to_string());

    let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
    let config_filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    let path = format!("{}/{}", config_path, config_filename);

    let content = fs::read_to_string(&path)
        .map_err(|_| ErrorResponse::new(ErrorCode::ConfigurationNotFound))?;

    match format.as_str() {
        "yaml" => Ok(http::Response::builder()
            .status(200)
            .header(CONTENT_TYPE, "application/yaml")
            .body(axum::body::Body::from(content))
            .unwrap()
            .into_response()),
        _ => {
            let config = RuntimeConfig::from_yaml_str(&content).map_err(ErrorResponse::internal)?;
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
    authorization::require(&req, Permission::ConfigurationsUpdate)?;

    let config: Config = extract_json(req).await?;
    let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
    let config_filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    save_configuration(config, &format!("{}/{}", config_path, config_filename))?;

    Ok(http::Response::builder()
        .status(200)
        .body(axum::body::Body::empty())
        .unwrap()
        .into_response())
}

fn save_configuration(config: Config, path: &str) -> Result<(), ErrorResponse> {
    let config = RuntimeConfig::from_raw(config).map_err(|error| {
        ErrorResponse::new(ErrorCode::ConfigurationInvalid).with_message(error.to_string())
    })?;
    config.raw.to_file(path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{save_configuration, update_configurations};
    use crate::api::admin::{
        AdminPrincipal,
        authorization::{AdminAuthorization, Permission},
    };
    use axum::body::Body;
    use gate::cfg::{Config, RuntimeConfig, SCHEMA};
    use http::{Request, StatusCode};

    #[tokio::test]
    async fn admin_input_requires_the_supported_explicit_schema() {
        for payload in [
            serde_json::json!({ "http": {} }),
            serde_json::json!({ "schema": null }),
            serde_json::json!({ "schema": "stargate/v2alpha1" }),
            serde_json::json!({ "schema": "stargate/v2" }),
        ] {
            let mut req = Request::builder()
                .method("PUT")
                .uri("/admin/configurations")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap();
            req.extensions_mut().insert(AdminAuthorization::admin_key(
                AdminPrincipal::AdminKey {
                    key_id: "schema-test".to_string(),
                },
                [Permission::ConfigurationsUpdate].into_iter().collect(),
            ));
            let error = update_configurations(req).await.unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
            assert!(error.message.contains("schema"));
        }
    }

    #[test]
    fn invalid_admin_submission_does_not_replace_the_saved_config() {
        let path = std::env::temp_dir().join(format!(
            "stargate-admin-config-{}.yaml",
            ulid::Ulid::generate()
        ));
        let path_str = path.to_str().unwrap();
        save_configuration(Config::default(), path_str).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert_eq!(
            RuntimeConfig::from_file(path_str).unwrap().raw.schema,
            SCHEMA
        );

        for schema in ["stargate/v2alpha1", "stargate/v2", ""] {
            let config = Config {
                schema: schema.to_string(),
                ..Config::default()
            };
            let error = save_configuration(config, path_str).unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
            assert!(error.message.contains(SCHEMA));
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
        std::fs::remove_file(path).unwrap();
    }
}
