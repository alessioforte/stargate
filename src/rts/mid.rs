use crate::err::{ErrorResponse, HttpError};
use crate::etc::ext::RequestExt;
use actix_web::http::header::{HeaderName, HeaderValue};
use actix_web::{
    body::{EitherBody, MessageBody},
    dev::{ServiceRequest, ServiceResponse},
    middleware::Next,
    web::Data,
};
use std::time::Duration;

// TODO: Add here audit logging and create audit context

pub async fn middleware<B: MessageBody + 'static>(
    sr: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<EitherBody<B>>, actix_web::Error> {
    let req = sr.request();
    // Rate limiting ----------------------------------------------------------
    let gate = req.app_data::<Data<gate::Gate>>().unwrap();
    let limiter = gate.limiter.load();

    let client_ip = req.get_client_ip();

    let limit_name = "default";
    let key = client_ip
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let key = format!("lim:{}", key);
    let decision = match limiter.check(limit_name, &key).await {
        Ok(decision) => decision,
        Err(e) => {
            tracing::error!("Rate limiter error: {}", e);
            return Err(actix_web::error::ErrorInternalServerError(
                "Rate limiter error",
            ));
        }
    };

    let limit = decision.limit.to_string();
    let remaining = decision.remaining.to_string();

    if decision.allowed {
        let mut res = next.call(sr).await?;
        res.headers_mut().insert(
            HeaderName::from_static("x-ratelimit-limit"),
            HeaderValue::from_str(&limit).unwrap(),
        );
        res.headers_mut().insert(
            HeaderName::from_static("x-ratelimit-remaining"),
            HeaderValue::from_str(&remaining).unwrap(),
        );
        Ok(res.map_into_left_body())
    } else {
        let retry_after = decision.retry_after.unwrap_or(Duration::from_secs(60));
        let retry_after = chrono::Duration::from_std(retry_after).unwrap();
        let retry_after = tools::duration_to_string(&retry_after);

        let mut res =
            ErrorResponse::from(HttpError::TooManyRequests("Too Many Requests".to_string()));
        res.insert_header("Retry-After", &retry_after)
            .insert_header("X-RateLimit-Limit", &limit)
            .insert_header("X-RateLimit-Remaining", &remaining);
        // .insert_header("X-RateLimit-Reset", &reset);

        Err(res.into())
    }
}
