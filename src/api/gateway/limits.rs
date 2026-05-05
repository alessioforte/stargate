use crate::err::{ErrorResponse, HttpError};
use crate::etc::sub::Subject;
use ::http::{HeaderMap, HeaderName, HeaderValue};
use gate::Gate;
use std::sync::Arc;

pub async fn apply_limits(
    gate: &Arc<Gate>,
    subject: Option<&Subject>,
    client_ip: &str,
    headers: &mut HeaderMap,
    rate_limit_override: Option<String>,
    quota_override: Option<(String, u64)>,
) -> Result<(), ErrorResponse> {
    let limiter = gate.limiter.load();
    let mut sub_key = client_ip.to_string();
    let mut limit_name = rate_limit_override
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let mut quota_name = quota_override.as_ref().map(|(name, _)| name.clone());
    let mut quota_cost = quota_override.as_ref().map(|(_, cost)| *cost).unwrap_or(1);

    if let Some(subject) = subject {
        if rate_limit_override.is_none() {
            if let Some(rate_limit) = subject
                .get_attr("rate_limit")
                .and_then(|value| value.as_str())
            {
                limit_name = rate_limit.to_string();
            }
        }

        if quota_override.is_none() {
            if let Some(quota) = subject.get_attr("quota").and_then(|value| value.as_str()) {
                quota_name = Some(quota.to_string());
                quota_cost = 1;
            }
        }

        sub_key = subject.id.clone();
    }

    let mut key = String::with_capacity(4 + sub_key.len());
    key.push_str("lim:");
    key.push_str(&sub_key);

    let decision = limiter
        .check(&limit_name, &key, None)
        .await
        .map_err(|error| {
            tracing::error!("Rate limiter error: {}", error);
            ErrorResponse::from(HttpError::InternalServerError(
                "Rate limiter error".to_string(),
            ))
        })?;

    let limit = decision.limit.to_string();
    let remaining = decision.remaining.to_string();

    if !decision.is_allowed() {
        let retry_after = decision
            .retry_after
            .unwrap_or(std::time::Duration::from_secs(60));
        let retry_after = retry_after_header_value(retry_after);
        let mut response =
            ErrorResponse::from(HttpError::TooManyRequests("Too Many Requests".to_string()));
        response
            .insert_header("retry-after", &retry_after)
            .insert_header("x-ratelimit-limit", &limit)
            .insert_header("x-ratelimit-remaining", &remaining);
        return Err(response);
    }

    headers.insert(
        HeaderName::from_static("x-ratelimit-limit"),
        HeaderValue::from_str(&limit).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("x-ratelimit-remaining"),
        HeaderValue::from_str(&remaining).unwrap(),
    );

    if let Some(quota_name) = quota_name {
        let mut quota_key = String::with_capacity(6 + sub_key.len());
        quota_key.push_str("quota:");
        quota_key.push_str(&sub_key);

        let decision = limiter
            .check(&quota_name, &quota_key, Some(quota_cost))
            .await
            .map_err(|error| {
                tracing::error!("Quota limiter error: {}", error);
                ErrorResponse::from(HttpError::InternalServerError(
                    "Quota limiter error".to_string(),
                ))
            })?;

        let limit = decision.limit.to_string();
        let remaining = decision.remaining.to_string();

        if !decision.is_allowed() {
            let retry_after = decision
                .retry_after
                .unwrap_or(std::time::Duration::from_secs(60));
            let retry_after = retry_after_header_value(retry_after);
            let mut response = ErrorResponse::from(HttpError::TooManyRequests(
                "Quota limit exceeded".to_string(),
            ));
            response
                .insert_header("retry-after", &retry_after)
                .insert_header("x-quota-limit", &limit)
                .insert_header("x-quota-remaining", &remaining);
            return Err(response);
        }

        headers.insert(
            HeaderName::from_static("x-quota-limit"),
            HeaderValue::from_str(&limit).unwrap(),
        );
        headers.insert(
            HeaderName::from_static("x-quota-remaining"),
            HeaderValue::from_str(&remaining).unwrap(),
        );
    }

    Ok(())
}

fn retry_after_header_value(retry_after: std::time::Duration) -> String {
    let retry_after = match chrono::Duration::from_std(retry_after) {
        Ok(retry_after) => retry_after,
        Err(error) => {
            tracing::warn!(
                error = ?error,
                "Retry-After duration overflowed chrono::Duration; defaulting to zero"
            );
            chrono::Duration::default()
        }
    };

    tools::duration_to_string(&retry_after)
}
