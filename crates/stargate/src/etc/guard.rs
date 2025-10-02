use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::etc::{ext::RequestExt, lim::RateLimiter, sub::Subject};
use actix_web::HttpRequest;
use db::Transaction;
use std::net::IpAddr;
use store::Store;

// TODO: maybe should return a Result instead of an Option
pub async fn verify_api_key(req: &HttpRequest) -> Option<Subject> {
    let api_key = match req.get_api_key() {
        Some(k) => k,
        None => {
            return None;
        }
    };
    let hash_key = apiks::hash_api_key(&api_key);
    let store = etc::store::use_store();
    let key = format!("sub:{}", hash_key);
    let session = store.get::<Subject>(&key).await.unwrap_or(None);

    if let Some(subject) = session {
        return Some(subject);
    }

    // if is not found in the store try to find it in the database
    let service = etc::db::service();
    let result = match service.get_api_key_by_hash(&hash_key).await {
        Ok(v) => v,
        Err(e) => {
            log::error!("Failed to get api key: {}", e);
            return None;
        }
    };

    if let Some(api_key) = result {
        if api_key.revoked {
            return None;
        }

        let subject: Option<Subject> = match service.get_subject_by_id(&api_key.id).await {
            Ok(subject) => subject.map(|s| s.into()),
            Err(e) => {
                log::error!("Failed to get subject by id: {}", e);
                return None;
            }
        };

        // store the subject in the session store
        if let Some(subject) = subject {
            // let ttl = api_key.exp.map(|exp| exp as u64);
            let ttl = Some(3600);
            match store.set(&key, &subject, ttl).await {
                Ok(_) => (),
                Err(e) => {
                    log::error!("Failed to store subject in the session store: {}", e);
                }
            }

            return Some(subject);
        }
    }

    None
}

pub async fn verify_jwt(req: &HttpRequest) -> Option<Subject> {
    let token = match req.get_token() {
        Some(t) => t,
        None => {
            return None;
        }
    };

    let jwt = etc::jwt::jwt_config();
    let claims = match jwt.validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return None;
        }
    };

    // Get subject from session store
    let store = etc::store::use_store();
    let sid = claims.sid.clone().unwrap_or_default();
    let key = format!("sub:{}", sid);
    let session = store.get::<Subject>(&key).await;

    if let Ok(Some(subject)) = session {
        return Some(subject);
    }

    None
}

pub async fn apply_rate_limit(
    limiter: &RateLimiter,
    sub: &Option<Subject>,
    ip: &Option<IpAddr>,
) -> Result<(), ErrorResponse> {
    let (rd, qd) = if let Some(subject) = sub {
        // if has auth get the subject config and pass it to the rate limiter
        // if has auth but the subject has no config use the default rate limit
        let config = subject.limits.clone();
        limiter
            .check_rate_limit_and_consume_quota(&subject.id, 1, config)
            .await
            .map_err(|e| ErrorResponse::from(HttpError::TooManyRequests(e.to_string())))?
    } else {
        // if has no auth apply rate limit to the client IP
        // if has no auth and no client IP (localhost) use the very restrictive default rate limit
        // TODO: Analyze how to handle if no IP is found
        limiter
            .check_rate_limit_and_consume_quota(
                &ip.map_or("anonymous".to_string(), |ip| ip.to_string()),
                1,
                None,
            )
            .await
            .map_err(|e| ErrorResponse::from(HttpError::TooManyRequests(e.to_string())))?
    };

    if !rd.allowed {
        let retry_after = rd.retry_after_ms.unwrap_or(60_000) / 1000; // in seconds
        let message = format!(
            "Too Many Requests. Please try again in {} seconds.",
            retry_after
        );
        return Err(ErrorResponse::from(HttpError::TooManyRequests(message))
            .insert_header("Retry-After".to_string(), retry_after.to_string())
            .insert_header("X-Ratelimit-After".to_string(), retry_after.to_string()));
    }

    if !qd.allowed {
        return Err(ErrorResponse::from(HttpError::TooManyRequests(
            "Quota exceeded".to_string(),
        )));
    }

    Ok(())
}
