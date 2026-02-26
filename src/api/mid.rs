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

pub async fn middleware<B: MessageBody + 'static>(
    sr: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<EitherBody<B>>, actix_web::Error> {
    let req = sr.request();
    // Rate limiting ----------------------------------------------------------
    let gate = req.app_data::<Data<gate::Gate>>().unwrap();
    let limiter = gate.limiter.load();

    let client_ip = req.get_client_ip();

    // for the stargate api is always "default" the limit to check
    let limit_name = "default";

    let mut key = String::with_capacity(4 + client_ip.len());
    key.push_str("lim:");
    key.push_str(&client_ip);
    let decision = match limiter.check(limit_name, &key, None).await {
        Ok(decision) => decision,
        Err(e) => {
            tracing::error!("Rate limiter error: {}", e);
            let res = ErrorResponse::from(HttpError::InternalServerError(
                "Rate limiter error".to_string(),
            ));
            return Err(res.into());
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

        Err(res.into())
    }
}
