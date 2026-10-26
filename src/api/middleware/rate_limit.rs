//! IAM/admin request limits.
//!
//! Every IAM route first charges the client IP's ingress allowance, the same
//! bucket the gateway uses, before any credential is read. Authenticated
//! `/admin/*` principals then charge a bucket of their own
//! ([`charge_admin_principal`]), so one principal's automation never drains
//! the budget of another principal behind the same address.

use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::gate::{Gate, RuntimeSnapshot, retain_runtime};
use crate::etc::limits::{ingress_key, insert_limit_headers, limited_response};
use crate::web::request::RequestExt;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::HeaderMap;
use lim::RateLimitDecision;
use std::sync::Arc;

/// Named limit charged per authenticated admin principal.
const ADMIN_PRINCIPAL_LIMIT: &str = "default";

pub async fn ingress_limit_middleware(mut req: Request, next: Next) -> Response {
    let gate = req
        .extensions()
        .get::<Arc<Gate>>()
        .cloned()
        .expect("Gate extension must be set");

    let runtime = gate.snapshot();
    req.extensions_mut().insert(runtime.clone());
    let client_ip = req.get_client_ip();
    if let Err(error) = check_ingress(&runtime, &client_ip).await {
        return error.into_response();
    }
    next.run(req)
        .await
        .map(|body| retain_runtime(body, runtime))
}

async fn check_ingress(runtime: &RuntimeSnapshot, client_ip: &str) -> Result<(), ErrorResponse> {
    let ingress = &runtime.ingress;
    let key = ingress_key(client_ip);
    let check = runtime.core.limiter.check(&ingress.limit, &key, None);
    let decision = match tokio::time::timeout(ingress.timeout, check).await {
        Ok(Ok(decision)) => decision,
        Ok(Err(error)) => {
            tracing::error!(%error, "IAM ingress limiter failed");
            return Err(unavailable("ingress"));
        }
        Err(_) => {
            tracing::warn!("IAM ingress limiter exceeded its store deadline");
            return Err(unavailable("ingress"));
        }
    };
    if decision.is_allowed() {
        return Ok(());
    }
    Err(limited_response(
        &decision,
        "ingress",
        ErrorCode::IamIngressRateLimitExceeded,
        "x-ratelimit",
    ))
}

/// Charges an authenticated admin principal's own `default` bucket. `key`
/// must identify the principal, never the client address.
pub async fn charge_admin_principal(
    runtime: &RuntimeSnapshot,
    key: &str,
) -> Result<RateLimitDecision, ErrorResponse> {
    let decision = runtime
        .core
        .limiter
        .check(ADMIN_PRINCIPAL_LIMIT, key, None)
        .await
        .map_err(|error| {
            tracing::error!(%error, "IAM principal limiter failed");
            unavailable("subject")
        })?;
    if decision.is_allowed() {
        return Ok(decision);
    }
    Err(limited_response(
        &decision,
        "subject",
        ErrorCode::IamRateLimitExceeded,
        "x-ratelimit",
    ))
}

pub fn insert_principal_limit_headers(headers: &mut HeaderMap, decision: &RateLimitDecision) {
    insert_limit_headers(headers, decision, "subject", "x-ratelimit");
}

fn unavailable(scope: &'static str) -> ErrorResponse {
    ErrorResponse::new(ErrorCode::IamLimiterUnavailable).with_param("scope", scope)
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::{charge_admin_principal, ingress_limit_middleware};
    use crate::etc::gate::{RuntimeSnapshot, prepare_config, test_support};
    use axum::{Extension, Router, body::Body, middleware::from_fn, routing::get};
    use http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tokio::sync::Barrier;
    use tower::ServiceExt;

    fn config(ingress_burst: u64, default_burst: u64) -> gate::cfg::Config {
        serde_json::from_value(serde_json::json!({
            "schema": gate::cfg::SCHEMA,
            "ingress": {"limit": "ingress", "timeout": "250ms"},
            "limits": {
                "ingress": {"strategy": "gcra", "params": {"max_burst": ingress_burst, "replenish_1_per": "1h"}},
                "default": {"strategy": "gcra", "params": {"max_burst": default_burst, "replenish_1_per": "1h"}},
            },
        }))
        .unwrap()
    }

    fn app(gate: Arc<crate::etc::gate::Gate>) -> Router {
        Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(from_fn(ingress_limit_middleware))
            .layer(Extension(gate))
    }

    fn request(peer: &str) -> Request<Body> {
        let mut req = Request::builder().uri("/").body(Body::empty()).unwrap();
        req.extensions_mut().insert(axum::extract::ConnectInfo(
            peer.parse::<std::net::SocketAddr>().unwrap(),
        ));
        req
    }

    async fn error_code(response: axum::response::Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["code"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn iam_middleware_and_downstream_reader_share_the_ingress_generation() {
        let gate = Arc::new(test_support::gate(config(100, 100)));
        let pinned = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let app = Router::new()
            .route(
                "/",
                get({
                    let pinned = pinned.clone();
                    let resume = resume.clone();
                    move |req: axum::extract::Request| {
                        let pinned = pinned.clone();
                        let resume = resume.clone();
                        async move {
                            if req.headers().contains_key("pause") {
                                pinned.wait().await;
                                resume.wait().await;
                            }
                            req.extensions()
                                .get::<Arc<RuntimeSnapshot>>()
                                .unwrap()
                                .version
                                .to_string()
                        }
                    }
                }),
            )
            .layer(from_fn(ingress_limit_middleware))
            .layer(Extension(gate.clone()));
        let pending = tokio::spawn(
            app.clone().oneshot(
                Request::builder()
                    .uri("/")
                    .header("pause", "true")
                    .body(Body::empty())
                    .unwrap(),
            ),
        );
        pinned.wait().await;
        gate.activate(prepare_config(config(100, 100)).unwrap())
            .unwrap();
        resume.wait().await;
        let response = pending.await.unwrap().unwrap();
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "0"
        );

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "1"
        );
    }

    #[tokio::test]
    async fn iam_routes_charge_the_ingress_allowance_not_the_default_limit() {
        // `default` would deny the second request; only ingress may apply.
        let gate = Arc::new(test_support::gate(config(2, 1)));
        let app = app(gate.clone());
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(request("203.0.113.1:1000"))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(!response.headers().contains_key("x-ratelimit-limit"));
        }

        let response = app
            .clone()
            .oneshot(request("203.0.113.1:1000"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()["x-ratelimit-scope"], "ingress");
        assert_eq!(response.headers()["x-ratelimit-remaining"], "0");
        assert!(response.headers().contains_key("retry-after"));
        assert_eq!(
            error_code(response).await,
            "iam.ingress_rate_limit_exceeded"
        );

        // Another address has its own allowance.
        let response = app.oneshot(request("203.0.113.2:1000")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_principals_have_independent_buckets_outside_the_ingress_namespace() {
        let gate = test_support::gate(config(1, 2));
        let runtime = gate.snapshot();
        for remaining in [1, 0] {
            let decision = charge_admin_principal(&runtime, "lim:admin:key:automation")
                .await
                .unwrap();
            assert_eq!(decision.remaining, remaining);
        }
        let error = charge_admin_principal(&runtime, "lim:admin:key:automation")
            .await
            .unwrap_err();
        assert_eq!(error.code, crate::err::ErrorCode::IamRateLimitExceeded);
        let response = axum::response::IntoResponse::into_response(error);
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()["x-ratelimit-scope"], "subject");
        assert!(response.headers().contains_key("retry-after"));

        // The exhausted automation key does not affect the console user, and
        // neither touches the per-IP ingress allowance.
        charge_admin_principal(&runtime, "lim:admin:user:console")
            .await
            .unwrap();
        let ingress = runtime
            .core
            .limiter
            .check(
                "ingress",
                &crate::etc::limits::ingress_key("127.0.0.1"),
                None,
            )
            .await
            .unwrap();
        assert!(ingress.is_allowed());
    }
}
