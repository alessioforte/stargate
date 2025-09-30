use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::ResetPeriod;

/// Usage tracking for a specific quota period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaUsage {
    /// Number of requests used in this period
    pub used: u64,
    /// The reset period for this quota
    pub reset_period: ResetPeriod, // TODO: maybe not needed here
    /// The quota limit
    pub limit: u64,
    /// Timestamp when this period started
    pub period_start: DateTime<Utc>,
    /// Timestamp when this period ends
    pub period_end: DateTime<Utc>,
    /// Last time this usage was updated
    pub last_updated: DateTime<Utc>,
}

impl QuotaUsage {
    /// Create a new quota usage tracker
    pub fn new(reset_period: ResetPeriod, limit: u64, reference_time: DateTime<Utc>) -> Self {
        let (period_start, period_end) = reset_period.period_bounds(reference_time);

        Self {
            used: 0,
            reset_period,
            limit,
            period_start,
            period_end,
            last_updated: reference_time,
        }
    }

    /// Check if this usage period is current (now falls within the period)
    pub fn is_current(&self, now: DateTime<Utc>) -> bool {
        now >= self.period_start && now < self.period_end
    }

    /// Check if this usage period has expired
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.period_end
    }

    /// Add usage to this period with overflow protection
    pub fn add_usage(&mut self, count: u64) {
        self.used = self.used.saturating_add(count);
        self.last_updated = Utc::now();
    }

    /// Reset usage for this period and update period bounds
    pub fn reset(&mut self, reference_time: DateTime<Utc>) {
        let (period_start, period_end) = self.reset_period.period_bounds(reference_time);
        self.used = 0;
        self.period_start = period_start;
        self.period_end = period_end;
        self.last_updated = reference_time;
    }

    /// Get remaining quota in this period
    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.used)
    }

    /// Check if quota is exhausted
    pub fn is_exhausted(&self) -> bool {
        self.used >= self.limit
    }

    /// Get utilization percentage (0.0 to 1.0)
    pub fn utilization(&self) -> f64 {
        if self.limit == 0 {
            1.0
        } else {
            (self.used as f64) / (self.limit as f64)
        }
    }

    /// Get time until reset
    pub fn time_until_reset(&self, now: DateTime<Utc>) -> chrono::Duration {
        if now >= self.period_end {
            chrono::Duration::zero()
        } else {
            self.period_end - now
        }
    }
}
