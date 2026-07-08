use crate::err::{ErrorResponse, HttpError};
use crate::etc::{sub::Subject, telemetry};
use ::http::{HeaderMap, HeaderName, HeaderValue};
use gate::Gate;
use std::sync::Arc;

const DEFAULT_RATE_LIMIT: &str = "default";

struct SelectedLimits {
    subject_key: String,
    rate_limit_name: String,
    quota_name: Option<String>,
    quota_cost: u64,
}

pub async fn apply_limits(
    gate: &Arc<Gate>,
    subject: Option<&Subject>,
    client_ip: &str,
    headers: &mut HeaderMap,
    rate_limit_override: Option<String>,
    quota_override: Option<(String, u64)>,
) -> Result<(), ErrorResponse> {
    let limiter = gate.limiter.load();
    let selected = select_limits(
        subject,
        client_ip,
        rate_limit_override.as_deref(),
        quota_override
            .as_ref()
            .map(|(name, cost)| (name.as_str(), *cost)),
    );

    let mut key = String::with_capacity(4 + selected.subject_key.len());
    key.push_str("lim:");
    key.push_str(&selected.subject_key);

    let decision = limiter
        .check(&selected.rate_limit_name, &key, None)
        .await
        .map_err(|error| {
            telemetry::record_gateway_policy("rate_limit", "error");
            tracing::error!("Rate limiter error: {}", error);
            ErrorResponse::from(HttpError::InternalServerError(
                "Rate limiter error".to_string(),
            ))
        })?;

    let limit = decision.limit.to_string();
    let remaining = decision.remaining.to_string();

    if !decision.is_allowed() {
        telemetry::record_gateway_policy("rate_limit", "denied");
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
    telemetry::record_gateway_policy("rate_limit", "allowed");

    headers.insert(
        HeaderName::from_static("x-ratelimit-limit"),
        HeaderValue::from_str(&limit).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("x-ratelimit-remaining"),
        HeaderValue::from_str(&remaining).unwrap(),
    );

    if let Some(quota_name) = selected.quota_name {
        let mut quota_key = String::with_capacity(6 + selected.subject_key.len());
        quota_key.push_str("quota:");
        quota_key.push_str(&selected.subject_key);

        let decision = limiter
            .check(&quota_name, &quota_key, Some(selected.quota_cost))
            .await
            .map_err(|error| {
                telemetry::record_gateway_policy("quota", "error");
                tracing::error!("Quota limiter error: {}", error);
                ErrorResponse::from(HttpError::InternalServerError(
                    "Quota limiter error".to_string(),
                ))
            })?;

        let limit = decision.limit.to_string();
        let remaining = decision.remaining.to_string();

        if !decision.is_allowed() {
            telemetry::record_gateway_policy("quota", "denied");
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
        telemetry::record_gateway_policy("quota", "allowed");

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

fn select_limits(
    subject: Option<&Subject>,
    client_ip: &str,
    rate_limit_override: Option<&str>,
    quota_override: Option<(&str, u64)>,
) -> SelectedLimits {
    let mut rate_limit_name = rate_limit_override
        .unwrap_or(DEFAULT_RATE_LIMIT)
        .to_string();
    let mut quota_name = quota_override.map(|(name, _)| name.to_string());
    let mut quota_cost = quota_override.map(|(_, cost)| cost).unwrap_or(1);
    let mut subject_key = client_ip.to_string();

    if let Some(subject) = subject {
        if rate_limit_name == DEFAULT_RATE_LIMIT
            && let Some(rate_limit) = subject
                .get_attr("rate_limit")
                .and_then(|value| value.as_str())
        {
            rate_limit_name = rate_limit.to_string();
        }

        if quota_name.is_none()
            && let Some(quota) = subject.get_attr("quota").and_then(|value| value.as_str())
        {
            quota_name = Some(quota.to_string());
            quota_cost = 1;
        }

        subject_key = subject.id.clone();
    }

    SelectedLimits {
        subject_key,
        rate_limit_name,
        quota_name,
        quota_cost,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etc::sub::{Subject, SubjectType};
    use serde_json::json;

    fn subject(attrs: serde_json::Value) -> Subject {
        Subject {
            id: "sub_123".to_string(),
            sub_type: SubjectType::ApiKey,
            org_id: None,
            attrs,
        }
    }

    #[test]
    fn selects_default_rate_limit_for_anonymous_requests() {
        let selected = select_limits(None, "203.0.113.10", None, None);

        assert_eq!(selected.subject_key, "203.0.113.10");
        assert_eq!(selected.rate_limit_name, "default");
        assert_eq!(selected.quota_name, None);
        assert_eq!(selected.quota_cost, 1);
    }

    #[test]
    fn subject_attrs_override_implicit_default_rate_limit() {
        let subject = subject(json!({
            "rate_limit": "premium",
            "quota": "monthly-premium"
        }));

        let selected = select_limits(Some(&subject), "203.0.113.10", None, None);

        assert_eq!(selected.subject_key, "sub_123");
        assert_eq!(selected.rate_limit_name, "premium");
        assert_eq!(selected.quota_name, Some("monthly-premium".to_string()));
        assert_eq!(selected.quota_cost, 1);
    }

    #[test]
    fn subject_attrs_override_explicit_default_rate_limit_policy() {
        let subject = subject(json!({
            "rate_limit": "premium"
        }));

        let selected = select_limits(Some(&subject), "203.0.113.10", Some("default"), None);

        assert_eq!(selected.rate_limit_name, "premium");
    }

    #[test]
    fn resource_rate_limit_policy_overrides_subject_rate_limit_attr() {
        let subject = subject(json!({
            "rate_limit": "premium"
        }));

        let selected = select_limits(Some(&subject), "203.0.113.10", Some("reports"), None);

        assert_eq!(selected.rate_limit_name, "reports");
    }

    #[test]
    fn quota_policy_overrides_subject_quota_attr_and_preserves_cost() {
        let subject = subject(json!({
            "quota": "monthly-premium"
        }));

        let selected = select_limits(
            Some(&subject),
            "203.0.113.10",
            None,
            Some(("daily-reports", 10)),
        );

        assert_eq!(selected.rate_limit_name, "default");
        assert_eq!(selected.quota_name, Some("daily-reports".to_string()));
        assert_eq!(selected.quota_cost, 10);
    }
}
