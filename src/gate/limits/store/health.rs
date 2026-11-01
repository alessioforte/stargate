use super::LimitPoint;
use crate::obs::telemetry;
use gate::cfg::OnStoreError;
use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard},
    time::Duration,
};
use tokio::time::Instant;

const DEGRADED_WINDOW: Duration = Duration::from_secs(60);
const WARNING_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct PointHealth {
    last_error: Option<Instant>,
    last_warning: Option<Instant>,
    recovery_log_pending: bool,
}

impl PointHealth {
    fn is_degraded(&self, now: Instant) -> bool {
        self.last_error
            .is_some_and(|last| now.duration_since(last) < DEGRADED_WINDOW)
    }

    fn failure(&mut self, now: Instant) -> bool {
        let entering = !self.is_degraded(now);
        self.last_error = Some(now);
        let can_warn = self
            .last_warning
            .is_none_or(|last| now.duration_since(last) >= WARNING_INTERVAL);
        if entering && can_warn {
            self.last_warning = Some(now);
            self.recovery_log_pending = true;
            true
        } else {
            false
        }
    }

    fn success(&mut self) -> bool {
        self.last_error = None;
        // Pair recovery messages with emitted warnings, including when a
        // suppressed intermittent failure has already cleared again.
        std::mem::take(&mut self.recovery_log_pending)
    }
}

/// Process-lifetime diagnostics. Exhaustive field selection makes a new
/// application point a compile-time change, never an unchecked array index.
#[derive(Default)]
pub(crate) struct LimitStoreHealth {
    gateway_ingress: Mutex<PointHealth>,
    iam_ingress: Mutex<PointHealth>,
    admin_principal: Mutex<PointHealth>,
    rate_limit: Mutex<PointHealth>,
    quota: Mutex<PointHealth>,
}

impl LimitStoreHealth {
    fn point(&self, point: LimitPoint) -> MutexGuard<'_, PointHealth> {
        match point {
            LimitPoint::GatewayIngress => &self.gateway_ingress,
            LimitPoint::IamIngress => &self.iam_ingress,
            LimitPoint::AdminPrincipal => &self.admin_principal,
            LimitPoint::RateLimit => &self.rate_limit,
            LimitPoint::Quota => &self.quota,
        }
        .lock()
        .expect("Limit store health lock poisoned")
    }

    /// Each value means a store failure within the last minute without a
    /// subsequent successful check. Expiry needs no traffic or background task.
    pub(crate) fn degraded_points(&self) -> BTreeMap<&'static str, bool> {
        let now = Instant::now();
        LimitPoint::ALL
            .iter()
            .map(|&point| (point.name(), self.point(point).is_degraded(now)))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn is_degraded(&self) -> bool {
        self.degraded_points().values().any(|degraded| *degraded)
    }

    pub(super) fn success(&self, point: LimitPoint) {
        let mut state = self.point(point);
        if state.success() {
            tracing::info!(point = point.name(), "Limit store recovered");
        }
    }

    pub(super) fn failure(&self, point: LimitPoint, policy: OnStoreError, reason: &'static str) {
        let outcome = match policy {
            OnStoreError::Allow => "allowed",
            OnStoreError::Deny => "denied",
        };
        // Metrics count every failure, independently of diagnostic/log windows.
        telemetry::record_limit_store_error(point.name(), outcome);
        let mut state = self.point(point);
        if state.failure(Instant::now()) {
            tracing::warn!(
                point = point.name(),
                outcome,
                reason,
                "Limit store degraded"
            );
        }
    }
}

#[cfg(test)]
mod tests;
