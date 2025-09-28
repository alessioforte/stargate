use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::ResetPeriod;

/// Rate limit decision with additional metadata
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitDecision {
    /// Whether the request is allowed
    pub allowed: bool,

    /// Remaining tokens in the bucket
    pub remaining_tokens: f64,

    /// Maximum tokens (burst size)
    pub max_tokens: f64,

    /// Estimated time until next token is available (in milliseconds)
    pub retry_after_ms: Option<u64>,
    // /// The rate limit configuration used
    // pub config: crate::config::RateLimitConfig,
}

impl RateLimitDecision {
    /// Create an allowed decision
    pub fn allowed(
        remaining_tokens: f64,
        max_tokens: f64,
        // config: crate::config::RateLimitConfig,
    ) -> Self {
        Self {
            allowed: true,
            remaining_tokens,
            max_tokens,
            retry_after_ms: None,
            // config,
        }
    }

    /// Create a denied decision
    pub fn denied(
        remaining_tokens: f64,
        max_tokens: f64,
        retry_after_ms: u64,
        // config: crate::config::RateLimitConfig,
    ) -> Self {
        Self {
            allowed: false,
            remaining_tokens,
            max_tokens,
            retry_after_ms: Some(retry_after_ms),
            // config,
        }
    }

    /// Get retry after duration if available
    pub fn retry_after_duration(&self) -> Option<std::time::Duration> {
        self.retry_after_ms
            .map(|ms| std::time::Duration::from_millis(ms))
    }

    /// Get the utilization percentage (0.0 to 1.0)
    pub fn utilization(&self) -> f64 {
        if self.max_tokens == 0.0 {
            0.0
        } else {
            1.0 - (self.remaining_tokens / self.max_tokens)
        }
    }
}

/// Type of quota violation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum QuotaViolationType {
    Quota(ResetPeriod),
}

impl QuotaViolationType {
    /// Check if this is a daily violation
    pub fn is_daily(&self) -> bool {
        matches!(self, QuotaViolationType::Quota(ResetPeriod::Daily))
    }

    /// Check if this is a monthly violation
    pub fn is_monthly(&self) -> bool {
        matches!(self, QuotaViolationType::Quota(ResetPeriod::Monthly))
    }

    /// Check if this is a weekly violation
    pub fn is_weekly(&self) -> bool {
        matches!(self, QuotaViolationType::Quota(ResetPeriod::Weekly))
    }

    /// Check if this is a yearly violation
    pub fn is_yearly(&self) -> bool {
        matches!(self, QuotaViolationType::Quota(ResetPeriod::Yearly))
    }

    /// Check if this is a permanent violation
    pub fn is_permanent(&self) -> bool {
        matches!(self, QuotaViolationType::Quota(ResetPeriod::Never))
    }
}

/// Status for a specific quota period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaPeriodStatus {
    /// Number of requests used
    pub used: u64,
    /// Total limit for this period
    pub limit: u64,
    /// Remaining requests in this period
    pub remaining: u64,
    /// When this period resets
    pub reset_time: DateTime<Utc>,
}

impl QuotaPeriodStatus {
    /// Get utilization percentage (0.0 to 1.0)
    pub fn utilization(&self) -> f64 {
        if self.limit == 0 {
            1.0
        } else {
            (self.used as f64) / (self.limit as f64)
        }
    }

    /// Check if quota is exhausted
    pub fn is_exhausted(&self) -> bool {
        self.used >= self.limit
    }

    /// Get time until reset
    pub fn time_until_reset(&self, now: DateTime<Utc>) -> chrono::Duration {
        if now >= self.reset_time {
            chrono::Duration::zero()
        } else {
            self.reset_time - now
        }
    }
}

/// Overall quota status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaStatus {
    /// Status for each configured period
    pub periods: std::collections::HashMap<ResetPeriod, QuotaPeriodStatus>,
}

impl QuotaStatus {
    /// Check if any quota is exhausted
    pub fn is_any_exhausted(&self) -> bool {
        self.periods.values().any(|status| status.is_exhausted())
    }

    /// Get the next reset time across all periods
    pub fn next_reset_time(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.periods
            .values()
            .map(|status| status.reset_time)
            .filter(|&reset_time| reset_time > now)
            .min()
    }

    /// Get maximum utilization across all quotas
    pub fn max_utilization(&self) -> f64 {
        self.periods
            .values()
            .map(|status| status.utilization())
            .fold(0.0, f64::max)
    }

    /// Get status for a specific reset period
    pub fn get_period_status(&self, period: &ResetPeriod) -> Option<&QuotaPeriodStatus> {
        self.periods.get(period)
    }
}

/// Decision result from quota checking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaDecision {
    /// Whether the request is allowed
    pub allowed: bool,
    /// Type of violation if denied
    pub violation_type: Option<QuotaViolationType>,
    /// Current quota status
    pub status: QuotaStatus,
    /// Current usage (for the violated quota if denied)
    pub used: Option<u64>,
    /// Limit (for the violated quota if denied)
    pub limit: Option<u64>,
    /// Time to wait before retry if denied
    pub retry_after: Option<chrono::Duration>,
}

impl QuotaDecision {
    /// Create an allowed decision
    pub fn allowed(status: QuotaStatus) -> Self {
        Self {
            allowed: true,
            violation_type: None,
            status,
            used: None,
            limit: None,
            retry_after: None,
        }
    }

    /// Create a denied decision
    pub fn denied(
        violation_type: QuotaViolationType,
        status: QuotaStatus,
        used: u64,
        limit: u64,
        retry_after: chrono::Duration,
    ) -> Self {
        Self {
            allowed: false,
            violation_type: Some(violation_type),
            status,
            used: Some(used),
            limit: Some(limit),
            retry_after: Some(retry_after),
        }
    }

    /// Get retry after duration
    pub fn retry_after_duration(&self) -> Option<chrono::Duration> {
        self.retry_after
    }

    /// Check if this is a daily violation
    pub fn is_daily_violation(&self) -> bool {
        self.violation_type.as_ref().map_or(false, |v| v.is_daily())
    }

    /// Check if this is a monthly violation
    pub fn is_monthly_violation(&self) -> bool {
        self.violation_type
            .as_ref()
            .map_or(false, |v| v.is_monthly())
    }

    /// Check if this is a weekly violation
    pub fn is_weekly_violation(&self) -> bool {
        self.violation_type
            .as_ref()
            .map_or(false, |v| v.is_weekly())
    }

    /// Check if this is a yearly violation
    pub fn is_yearly_violation(&self) -> bool {
        self.violation_type
            .as_ref()
            .map_or(false, |v| v.is_yearly())
    }

    /// Check if this is a permanent quota violation
    pub fn is_permanent_violation(&self) -> bool {
        self.violation_type
            .as_ref()
            .map_or(false, |v| v.is_permanent())
    }
}
