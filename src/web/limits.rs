use crate::{
    err::{ErrorCode, ErrorResponse},
    web::headers::retry_after_header_value,
};
use http::{HeaderMap, HeaderName, HeaderValue};
use lim::RateLimitDecision;

/// A `429` carrying `Retry-After` and the denying check's `{prefix}-*` headers.
pub fn limited_response(
    decision: &RateLimitDecision,
    scope: &'static str,
    code: ErrorCode,
    header_prefix: &str,
) -> ErrorResponse {
    let retry_after = retry_after_header_value(decision.retry_after);
    let mut response = ErrorResponse::new(code);
    response
        .insert_header("retry-after", &retry_after)
        .insert_header(
            &format!("{header_prefix}-limit"),
            &decision.limit.to_string(),
        )
        .insert_header(
            &format!("{header_prefix}-remaining"),
            &decision.remaining.to_string(),
        )
        .insert_header(&format!("{header_prefix}-scope"), scope);
    response
}

pub fn insert_limit_headers(
    headers: &mut HeaderMap,
    decision: &RateLimitDecision,
    scope: &'static str,
    prefix: &str,
) {
    let entries = [
        (format!("{prefix}-limit"), decision.limit.to_string()),
        (
            format!("{prefix}-remaining"),
            decision.remaining.to_string(),
        ),
        (format!("{prefix}-scope"), scope.to_string()),
    ];
    for (name, value) in entries {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            headers.insert(name, value);
        }
    }
}
