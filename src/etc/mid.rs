use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::ext::RequestExt;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

pub async fn rate_limit_middleware(req: Request, next: Next) -> Response {
    let gate = req
        .extensions()
        .get::<Arc<gate::Gate>>()
        .cloned()
        .expect("Gate extension must be set");

    let limiter = gate.limiter.load_full();
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
        response
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
