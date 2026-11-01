//! Store failure policy shared by gateway and IAM traffic admission.
use crate::{
    err::{ErrorCode, ErrorResponse},
    gate::RuntimeSnapshot,
};
use gate::cfg::OnStoreError;
use lim::{LimitKind, RateLimitDecision, RateLimitError};
mod health;
pub(crate) use health::LimitStoreHealth;

#[derive(Clone, Copy)]
pub(crate) enum LimitPoint {
    GatewayIngress,
    IamIngress,
    AdminPrincipal,
    RateLimit,
    Quota,
}

impl LimitPoint {
    const ALL: &'static [Self] = &[
        Self::GatewayIngress,
        Self::IamIngress,
        Self::AdminPrincipal,
        Self::RateLimit,
        Self::Quota,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::GatewayIngress => "gateway_ingress",
            Self::IamIngress => "iam_ingress",
            Self::AdminPrincipal => "admin_principal",
            Self::RateLimit => "rate_limit",
            Self::Quota => "quota",
        }
    }

    fn unavailable(self) -> ErrorCode {
        match self {
            Self::GatewayIngress => ErrorCode::GatewayIngressUnavailable,
            Self::IamIngress | Self::AdminPrincipal => ErrorCode::IamLimiterUnavailable,
            Self::RateLimit | Self::Quota => ErrorCode::GatewayLimiterUnavailable,
        }
    }

    fn kind(self) -> LimitKind {
        match self {
            Self::Quota => LimitKind::Quota,
            _ => LimitKind::Rate,
        }
    }
}

pub(crate) struct TrafficLimit<'a> {
    pub name: &'a str,
    pub key: &'a str,
    pub scope: &'static str,
    pub cost: Option<u64>,
}

/// `None` means the store check failed open: no allowance was obtained, so
/// callers must not fabricate a decision or emit headers for this check.
pub(crate) async fn check_traffic_limit(
    runtime: &RuntimeSnapshot,
    point: LimitPoint,
    policy: OnStoreError,
    check: TrafficLimit<'_>,
) -> Result<Option<RateLimitDecision>, ErrorResponse> {
    let limiter = &runtime.core.limiter;
    super::validate_limit(limiter, check.name, point.kind(), check.scope)?;
    // Redis bounds command responses, but awaits reconnection before sending.
    // Bound the entire check (including that wait) for every traffic point.
    let reason = match tokio::time::timeout(
        runtime.ingress.timeout,
        limiter.check(check.name, check.key, check.cost),
    )
    .await
    {
        Ok(Ok(decision)) => {
            runtime.limit_store_health.success(point);
            return Ok(Some(decision));
        }
        Ok(Err(error)) => match error {
            RateLimitError::UnknownLimit(_)
            | RateLimitError::IncompatibleLimit { .. }
            | RateLimitError::InvalidConfig(_) => {
                return Err(super::configuration_error(
                    check.name,
                    point.kind(),
                    check.scope,
                    &error,
                ));
            }
            RateLimitError::ClockError(_) | RateLimitError::RateLimitExceeded => {
                return Err(ErrorResponse::internal(error));
            }
            RateLimitError::BackendError(_) => {
                tracing::debug!(point = point.name(), %error, "Limit store check failed");
                "backend"
            }
            #[cfg(feature = "redis")]
            RateLimitError::RedisError(_) => {
                tracing::debug!(point = point.name(), %error, "Limit store check failed");
                "backend"
            }
            #[cfg(feature = "memory")]
            RateLimitError::MemoryError(_) => {
                tracing::debug!(point = point.name(), %error, "Limit store check failed");
                "backend"
            }
        },
        Err(_) => "timeout",
    };
    runtime.limit_store_health.failure(point, policy, reason);
    match policy {
        OnStoreError::Allow => Ok(None),
        OnStoreError::Deny => Err(ErrorResponse::new(point.unavailable())
            .with_param("scope", check.scope)
            .with_param("point", point.name())),
    }
}

#[cfg(test)]
mod tests;
