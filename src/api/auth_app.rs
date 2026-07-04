use axum::Router;
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

const DEFAULT_AUTH_APP_BASE_PATH: &str = "/auth";
const DEFAULT_AUTH_APP_DIR: &str = ".stargate/apps/auth";

pub fn router() -> Router {
    let base_path = std::env::var("AUTH_APP_BASE_PATH")
        .map(|value| normalize_base_path(&value))
        .unwrap_or_else(|_| DEFAULT_AUTH_APP_BASE_PATH.to_string());
    let app_dir = std::env::var_os("AUTH_APP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_AUTH_APP_DIR));
    router_from(base_path, app_dir)
}

fn router_from(base_path: String, app_dir: PathBuf) -> Router {
    let index = app_dir.join("index.html");
    let service = ServeDir::new(app_dir).fallback(ServeFile::new(index));

    Router::new().nest_service(&base_path, service)
}

fn normalize_base_path(value: &str) -> String {
    let trimmed = value.trim().trim_matches('/');
    if trimmed.is_empty() {
        return DEFAULT_AUTH_APP_BASE_PATH.to_string();
    }
    format!("/{trimmed}")
}
