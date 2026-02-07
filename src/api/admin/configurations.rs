use crate::err::{ErrorResponse, HttpError};
use actix_web::{HttpResponse, get, put, web};
use actix_web_grants::protect;
use gate::cfg::Config;
use serde::Deserialize;
use std::env;
use std::fs;

#[derive(Deserialize)]
struct Params {
    format: Option<String>,
}

#[utoipa::path(
    context_path = "/admin",
    path = "/configurations",
    tags = ["Admin"],
    responses(
        (status = 200, description = "OK")
    )
)]
#[get("")]
#[protect(any("super_admin", "configurations"))]
pub async fn get_configurations(query: web::Query<Params>) -> Result<HttpResponse, ErrorResponse> {
    let format = query.format.clone().unwrap_or("json".to_string());
    let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
    let config_filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    let path = format!("{}/{}", config_path, config_filename);
    let content = fs::read_to_string(path);
    let content = match content {
        Ok(content) => content,
        Err(_) => {
            return Err(HttpError::NotFound("Config file not found".to_string()).into());
        }
    };

    match format.as_str() {
        "yaml" => Ok(HttpResponse::Ok()
            .content_type("application/yaml")
            .body(content)),
        _ => {
            let config: Config = serde_yaml_bw::from_str(&content).map_err(|e| {
                HttpError::InternalServerError(format!("Failed to parse config file: {}", e))
            })?;
            let json_data = serde_json::to_string(&config).unwrap();
            Ok(HttpResponse::Ok()
                .content_type("application/json")
                .body(json_data))
        }
    }
}

#[utoipa::path(
    context_path = "/admin",
    path = "/configurations",
    tags = ["Admin"],
    responses(
        (status = 200, description = "OK")
    )
)]
#[put("")]
#[protect(any("super_admin", "configurations"))]
pub async fn update_configurations(
    config: web::Json<Config>,
) -> Result<HttpResponse, ErrorResponse> {
    let cfg = config.into_inner();
    cfg.to_file(&format!(
        "{}/{}",
        env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string()),
        env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string())
    ));
    Ok(HttpResponse::Ok().finish())
}

pub fn routes() -> actix_web::Scope {
    web::scope("/configurations")
        .service(get_configurations)
        .service(update_configurations)
}
