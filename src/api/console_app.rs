use axum::Router;
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

const DEFAULT_CONSOLE_APP_BASE_PATH: &str = "/stargate";
const DEFAULT_CONSOLE_APP_DIR: &str = ".stargate/apps/console";

pub fn router() -> Router {
    let base_path = std::env::var("CONSOLE_APP_BASE_PATH")
        .map(|value| normalize_base_path(&value))
        .unwrap_or_else(|_| DEFAULT_CONSOLE_APP_BASE_PATH.to_string());
    let app_dir = std::env::var_os("CONSOLE_APP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONSOLE_APP_DIR));
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
        return DEFAULT_CONSOLE_APP_BASE_PATH.to_string();
    }
    format!("/{trimmed}")
}

#[cfg(test)]
mod tests {
    use super::{normalize_base_path, router_from};
    use axum::body::Body;
    use http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    #[test]
    fn normalizes_console_app_base_path() {
        assert_eq!(normalize_base_path("stargate"), "/stargate");
        assert_eq!(normalize_base_path("/stargate/"), "/stargate");
        assert_eq!(normalize_base_path("  console/admin  "), "/console/admin");
    }

    #[tokio::test]
    async fn serves_console_spa_under_base_path() {
        let dir = temp_console_app_dir();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "console-index").unwrap();
        std::fs::write(dir.join("assets/app.js"), "console.log('ok');").unwrap();

        let app = router_from("/stargate".to_string(), dir.clone());

        let spa = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/stargate/users")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(spa.status(), StatusCode::OK);
        let body = spa.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"console-index");

        let asset = app
            .oneshot(
                Request::builder()
                    .uri("/stargate/assets/app.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);
        let body = asset.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"console.log('ok');");

        std::fs::remove_dir_all(dir).unwrap();
    }

    fn temp_console_app_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "stargate-console-app-test-{}-{nanos}",
            std::process::id()
        ))
    }
}
