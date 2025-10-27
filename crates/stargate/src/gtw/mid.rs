use crate::err::{ErrorResponse, HttpError};
use crate::etc::{ext::RequestExt, guard};
use actix_web::http::header::{HeaderName, HeaderValue};
use actix_web::{
    HttpMessage,
    body::{EitherBody, MessageBody},
    dev::{ServiceRequest, ServiceResponse},
    middleware::Next,
    web::Data,
};

pub async fn middleware<B: MessageBody + 'static>(
    sr: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<EitherBody<B>>, actix_web::Error> {
    let gate = sr.app_data::<Data<gate::Gate>>().unwrap();
    let req = sr.request();
    let client_ip = req.get_client_ip();

    let mut sub = match guard::verify_api_key(&req).await {
        Some(s) => Some(s),
        None => None,
    };

    if sub.is_none() {
        sub = match guard::verify_jwt(&req).await {
            Some(s) => Some(s),
            None => None,
        };
    }

    sr.extensions_mut().insert(sub);

    // Rate limiting ----------------------------------------------------------
    let limiter = gate.limiter.load();

    // TODO: get limit name from subject if it exists or apply default
    let limit_name = "default";
    let key = client_ip
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let key = format!("lim:{}", key);
    // TODO: handle the case when there is no limiter configured
    let decision = match limiter.check(limit_name, &key).await {
        Ok(decision) => decision,
        Err(e) => {
            log::error!("Rate limiter error: {}", e);
            return Err(actix_web::error::ErrorInternalServerError(
                "Rate limiter error",
            ));
        }
    };

    let limit = decision.limit.to_string();
    let remaining = decision.remaining.to_string();
    let reset = decision.reset_time.to_string();

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
        let retry_after = decision
            .retry_after
            .unwrap_or(std::time::Duration::from_secs(60));
        let retry_after = chrono::Duration::from_std(retry_after).unwrap();
        let retry_after = tools::duration_to_string(&retry_after);

        let mut res =
            ErrorResponse::from(HttpError::TooManyRequests("Too Many Requests".to_string()));
        res.insert_header("Retry-After", &retry_after)
            .insert_header("X-RateLimit-Limit", &limit)
            .insert_header("X-RateLimit-Remaining", &remaining)
            .insert_header("X-RateLimit-Reset", &reset);

        Err(res.into())
    }
}
