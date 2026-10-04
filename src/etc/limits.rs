use crate::{
    err::{ErrorCode, ErrorResponse},
    etc::telemetry,
};
use lim::{LimitKind, Limiter, RateLimitError};

pub fn attribute_name<'a>(
    attrs: &'a serde_json::Value,
    field: &str,
    kind: LimitKind,
    scope: &'static str,
) -> Result<Option<&'a str>, ErrorResponse> {
    match attrs.get(field) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(name)) => Ok(Some(name)),
        Some(_) => {
            let policy_type = kind.to_string();
            telemetry::record_gateway_policy(&policy_type, "error");
            tracing::warn!(
                field,
                scope,
                policy_type,
                "Selected limit attribute must be a name or null"
            );
            Err(
                ErrorResponse::new(ErrorCode::GatewayLimitConfigurationInvalid)
                    .with_param("field", format!("attrs.{field}"))
                    .with_param("policyType", policy_type)
                    .with_param("scope", scope)
                    .with_param("reason", "invalid_attribute"),
            )
        }
    }
}

pub fn validate_limit(
    limiter: &Limiter,
    name: &str,
    kind: LimitKind,
    scope: &'static str,
) -> Result<(), ErrorResponse> {
    limiter
        .validate_limit(name, kind)
        .map_err(|error| configuration_error(name, kind, scope, &error))
}

pub fn configuration_error(
    name: &str,
    kind: LimitKind,
    scope: &'static str,
    error: &RateLimitError,
) -> ErrorResponse {
    let name: String = name.chars().take(64).collect();
    let reason = match error {
        RateLimitError::UnknownLimit(_) => "unknown_limit",
        RateLimitError::IncompatibleLimit { .. } => "incompatible_strategy",
        _ => "invalid_configuration",
    };
    let policy_kind = match kind {
        LimitKind::Rate => "rate_limit",
        LimitKind::Quota => "quota",
    };
    telemetry::record_gateway_policy(policy_kind, "error");
    // Names from stored attributes are diagnostic values, never metric labels.
    tracing::warn!(limit = ?name, policy_type = policy_kind, scope, reason, "Selected limit configuration is invalid");
    ErrorResponse::new(ErrorCode::GatewayLimitConfigurationInvalid)
        .with_param("limit", name)
        .with_param("policyType", policy_kind)
        .with_param("scope", scope)
        .with_param("reason", reason)
}
