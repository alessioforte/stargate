use crate::srv::health::{Lifecycle, check_dependencies};
use axum::{
    Json, Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::get,
};
use http::{StatusCode, header::CACHE_CONTROL};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct Probe {
    status: &'static str,
}

/// Merge after the API rate limiter so probes have no policy dependencies.
pub fn router(lifecycle: Lifecycle) -> Router {
    Router::new()
        .route("/livez", get(get_livez))
        .route("/readyz", get(get_readyz))
        .with_state(lifecycle)
}

#[utoipa::path(
    get,
    path = "/livez",
    tags = ["Health"],
    summary = "Process liveness",
    description = "Responds while the HTTP server is running, including during draining. No dependency checks, authentication, or rate limiting. Also suitable for startup probes because the listener opens after initialization.",
    responses((status = 200, description = "Process alive", body = Probe))
)]
pub async fn get_livez() -> Response {
    respond(StatusCode::OK, Probe { status: "alive" })
}

#[utoipa::path(
    get,
    path = "/readyz",
    tags = ["Health"],
    summary = "Traffic readiness",
    description = "Ready after initialization, before shutdown, and while SQL and (in cluster mode) Redis respond. Each dependency has a one-second timeout; checks run concurrently. No authentication or rate limiting.",
    responses(
        (status = 200, description = "Ready for traffic", body = Probe),
        (status = 503, description = "Not ready for traffic", body = Probe)
    )
)]
pub async fn get_readyz(State(lifecycle): State<Lifecycle>) -> Response {
    let ready = lifecycle
        .check_ready(async { check_dependencies().await.is_healthy() })
        .await;
    respond(
        readiness_status(ready),
        Probe {
            status: if ready { "ready" } else { "not_ready" },
        },
    )
}

fn readiness_status(ready: bool) -> StatusCode {
    if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

fn respond(status: StatusCode, body: impl Serialize) -> Response {
    (status, [(CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::{Method, Request, header::CONTENT_TYPE};
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;
    use utoipa::OpenApi;

    async fn request(app: Router, method: Method, path: &str) -> Response {
        app.oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("Authorization", "Bearer invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn probes_bypass_policies_and_keep_liveness_during_outages_and_shutdown() {
        let lifecycle = Lifecycle::default();
        // Deliberately omit Gate: entering the limiter or gateway would panic.
        let app = crate::api::server_router(lifecycle.clone());
        for phase in ["starting", "serving", "draining"] {
            match phase {
                "serving" => lifecycle.mark_ready(),
                "draining" => lifecycle.begin_shutdown(),
                _ => (),
            }
            // SQL and Redis are uninitialized; readiness must fail closed.
            for (path, status, body) in [
                ("/livez", StatusCode::OK, json!({"status": "alive"})),
                (
                    "/readyz",
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"status": "not_ready"}),
                ),
            ] {
                let response = request(app.clone(), Method::GET, path).await;
                assert_eq!(response.status(), status, "{phase}: {path}");
                assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
                assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
                assert!(!response.headers().contains_key("x-ratelimit-limit"));
                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                    body
                );

                let response = request(app.clone(), Method::HEAD, path).await;
                assert_eq!(response.status(), status);
                assert!(
                    response
                        .into_body()
                        .collect()
                        .await
                        .unwrap()
                        .to_bytes()
                        .is_empty()
                );
            }
        }
    }

    #[test]
    fn openapi_publishes_probe_contracts() {
        let doc = serde_json::to_value(crate::api::ApiDoc::openapi()).unwrap();
        assert!(doc["paths"]["/livez"]["get"]["responses"]["200"].is_object());
        for status in ["200", "503"] {
            assert!(doc["paths"]["/readyz"]["get"]["responses"][status].is_object());
        }
        assert!(doc["paths"]["/health"].is_null());
    }
}
