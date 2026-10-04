use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::ext::RequestExt;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

pub async fn rate_limit_middleware(mut req: Request, next: Next) -> Response {
    let gate = req
        .extensions()
        .get::<Arc<crate::etc::gate::Gate>>()
        .cloned()
        .expect("Gate extension must be set");

    let runtime = gate.snapshot();
    req.extensions_mut().insert(runtime.clone());
    let limiter = &runtime.core.limiter;
    let client_ip = req.get_client_ip();

    let mut key = String::with_capacity(4 + client_ip.len());
    key.push_str("lim:");
    key.push_str(&client_ip);

    let decision = match limiter.check("default", &key, None).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Rate limiter error: {}", e);
            return ErrorResponse::new(ErrorCode::GatewayLimiterUnavailable).into_response();
        }
    };

    let limit = decision.limit.to_string();
    let remaining = decision.remaining.to_string();

    if decision.allowed {
        let mut response = next.run(req).await;
        let headers = response.headers_mut();
        headers.insert(
            http::header::HeaderName::from_static("x-ratelimit-limit"),
            limit.parse().unwrap(),
        );
        headers.insert(
            http::header::HeaderName::from_static("x-ratelimit-remaining"),
            remaining.parse().unwrap(),
        );
        response.map(|body| crate::etc::gate::retain_runtime(body, runtime))
    } else {
        let retry_after = decision
            .retry_after
            .unwrap_or(std::time::Duration::from_secs(60));
        let retry_after = chrono::Duration::from_std(retry_after).unwrap();
        let retry_after_str = tools::duration_to_string(&retry_after);

        let mut err = ErrorResponse::new(ErrorCode::GatewayRateLimitExceeded);
        err.insert_header("Retry-After", &retry_after_str)
            .insert_header("X-RateLimit-Limit", &limit)
            .insert_header("X-RateLimit-Remaining", &remaining);
        err.into_response()
    }
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::rate_limit_middleware;
    use crate::etc::gate::{RuntimeSnapshot, prepare_config, test_support};
    use axum::{Extension, Router, body::Body, middleware::from_fn, routing::get};
    use http::Request;
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tokio::sync::Barrier;
    use tower::ServiceExt;

    fn config(capacity: u64) -> gate::cfg::Config {
        serde_json::from_value(serde_json::json!({
            "schema": gate::cfg::SCHEMA,
            "limits": {"default": {"strategy": "token_bucket", "params": {"capacity": capacity, "refill_rate": 1}}},
        })).unwrap()
    }

    #[tokio::test]
    async fn iam_middleware_and_downstream_reader_share_the_ingress_generation() {
        let gate = Arc::new(test_support::gate(config(7)));
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
            .layer(from_fn(rate_limit_middleware))
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
        gate.activate(prepare_config(config(70)).unwrap()).unwrap();
        resume.wait().await;
        let response = pending.await.unwrap().unwrap();
        assert_eq!(response.headers()["x-ratelimit-limit"], "7");
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "0"
        );

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.headers()["x-ratelimit-limit"], "70");
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "1"
        );
    }
}
