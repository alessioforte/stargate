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
    // The login page is prerendered by the app build
    // (apps/auth/scripts/prerender.mjs) so it works without JavaScript.
    // Builds without it fall back to the client-rendered index.
    let login = app_dir.join("login.html");
    let login = if login.is_file() {
        login
    } else {
        index.clone()
    };
    let service = ServeDir::new(app_dir).fallback(ServeFile::new(index));

    Router::new()
        .route_service(&format!("{base_path}/login"), ServeFile::new(login))
        .nest_service(&base_path, service)
}

fn normalize_base_path(value: &str) -> String {
    let trimmed = value.trim().trim_matches('/');
    if trimmed.is_empty() {
        return DEFAULT_AUTH_APP_BASE_PATH.to_string();
    }
    format!("/{trimmed}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    async fn body_of(router: Router, uri: &str) -> String {
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = router.oneshot(request).await.unwrap();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn app_dir(name: &str, with_login: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{name}-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "index").unwrap();
        if with_login {
            std::fs::write(dir.join("login.html"), "login").unwrap();
        }
        dir
    }

    #[tokio::test]
    async fn serves_prerendered_login_page_only_on_login_route() {
        let dir = app_dir("auth-app-login", true);
        let router = router_from("/auth".to_string(), dir.clone());

        assert_eq!(body_of(router.clone(), "/auth/login").await, "login");
        assert_eq!(
            body_of(router.clone(), "/auth/login?return_to=x").await,
            "login"
        );
        assert_eq!(body_of(router, "/auth/signup").await, "index");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn login_route_falls_back_to_index_without_prerender() {
        let dir = app_dir("auth-app-index", false);
        let router = router_from("/auth".to_string(), dir.clone());

        assert_eq!(body_of(router, "/auth/login").await, "index");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
