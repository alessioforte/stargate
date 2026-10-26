use crate::conf::apps::auth_app_base_path;
use axum::Router;
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

const DEFAULT_AUTH_APP_DIR: &str = ".stargate/apps/auth";

pub fn router() -> Router {
    let app_dir = std::env::var_os("AUTH_APP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_AUTH_APP_DIR));
    router_from(auth_app_base_path(), app_dir)
}

/// Pages the provider sends browsers to. The app build prerenders them
/// (apps/auth/scripts/prerender.mjs, same list) into `<route>.html` so their
/// forms work without JavaScript.
const PRERENDERED_ROUTES: [&str; 3] = ["login", "logout", "signed-out"];

fn router_from(base_path: String, app_dir: PathBuf) -> Router {
    let index = app_dir.join("index.html");
    let mut router = Router::new();
    for route in PRERENDERED_ROUTES {
        // Builds without a prerendered page fall back to the client-rendered index.
        let page = app_dir.join(format!("{route}.html"));
        let page = if page.is_file() { page } else { index.clone() };
        router = router.route_service(&format!("{base_path}/{route}"), ServeFile::new(page));
    }
    let service = ServeDir::new(app_dir).fallback(ServeFile::new(index));
    router.nest_service(&base_path, service)
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

    fn app_dir(name: &str, prerendered: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{name}-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "index").unwrap();
        if prerendered {
            for route in PRERENDERED_ROUTES {
                std::fs::write(dir.join(format!("{route}.html")), route).unwrap();
            }
        }
        dir
    }

    #[tokio::test]
    async fn serves_prerendered_pages_only_on_their_routes() {
        let dir = app_dir("auth-app-login", true);
        let router = router_from("/auth".to_string(), dir.clone());

        assert_eq!(body_of(router.clone(), "/auth/login").await, "login");
        assert_eq!(
            body_of(router.clone(), "/auth/login?return_to=x").await,
            "login"
        );
        assert_eq!(body_of(router.clone(), "/auth/logout").await, "logout");
        assert_eq!(
            body_of(router.clone(), "/auth/logout?error=logout_expired").await,
            "logout"
        );
        assert_eq!(
            body_of(router.clone(), "/auth/signed-out").await,
            "signed-out"
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
