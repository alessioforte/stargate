//! Store failure policy shared by gateway and IAM traffic admission.
use crate::{
    err::{ErrorCode, ErrorResponse},
    gate::RuntimeSnapshot,
    obs::telemetry,
};
use gate::cfg::OnStoreError;
use lim::{LimitKind, RateLimitDecision, RateLimitError};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy)]
pub(crate) enum LimitPoint {
    GatewayIngress,
    IamIngress,
    AdminPrincipal,
    RateLimit,
    Quota,
}

impl LimitPoint {
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

/// One indicator per application point, retained across configuration activation.
#[derive(Default)]
pub(crate) struct LimitStoreHealth {
    degraded: [AtomicBool; 5],
}

impl LimitStoreHealth {
    pub(crate) fn is_degraded(&self) -> bool {
        self.degraded
            .iter()
            .any(|state| state.load(Ordering::Relaxed))
    }

    fn success(&self, point: LimitPoint) {
        if self.degraded[point as usize].swap(false, Ordering::Relaxed) {
            tracing::info!(point = point.name(), "Limit store recovered");
        }
    }

    fn failure(&self, point: LimitPoint, policy: OnStoreError, reason: &'static str) {
        let outcome = match policy {
            OnStoreError::Allow => "allowed",
            OnStoreError::Deny => "denied",
        };
        telemetry::record_limit_store_error(point.name(), outcome);
        if !self.degraded[point as usize].swap(true, Ordering::Relaxed) {
            // No backend text: connection errors may contain infrastructure details.
            tracing::warn!(
                point = point.name(),
                outcome,
                reason,
                "Limit store degraded"
            );
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
        Ok(Err(error)) if !is_store_error(&error) => {
            return Err(super::configuration_error(
                check.name,
                point.kind(),
                check.scope,
                &error,
            ));
        }
        Ok(Err(_)) => "backend",
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

fn is_store_error(error: &RateLimitError) -> bool {
    match error {
        RateLimitError::BackendError(_) => true,
        #[cfg(feature = "redis")]
        RateLimitError::RedisError(_) => true,
        #[cfg(feature = "memory")]
        RateLimitError::MemoryError(_) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
