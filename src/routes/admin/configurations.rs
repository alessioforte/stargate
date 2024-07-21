use crate::config::config::Config;
use crate::errors::{ErrorResponse, HttpError};
use actix_web::{get, put, web, HttpResponse};
use actix_web_grants::protect;
use serde::Deserialize;
use std::env;
use std::fs;

#[derive(Deserialize)]
struct Params {
    format: Option<String>,
}

#[get("")]
#[protect("SUPER_ADMIN")]
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
            let config: Config =
                serde_yaml::from_str(&content).expect("Unable to parse config file");
            let json_data = serde_json::to_string(&config).unwrap();
            Ok(HttpResponse::Ok()
                .content_type("application/json")
                .body(json_data))
        }
    }
}

#[put("")]
#[protect("SUPER_ADMIN")]
pub async fn update_configurations() -> Result<HttpResponse, ErrorResponse> {
    Ok(HttpResponse::Ok().json(web::Json("impl update configurations")))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/configurations")
        .service(get_configurations)
        .service(update_configurations)
}
