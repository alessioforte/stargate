use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{RateLimitError, Result};

/// Reset period for quota limits
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ResetPeriod {
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Never,
}

impl ResetPeriod {
    /// Calculate the start and end times for this reset period
    pub fn period_bounds(&self, reference_time: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
        match self {
            ResetPeriod::Daily => {
                let start = reference_time
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .map(|naive| Utc.from_utc_datetime(&naive))
                    .unwrap_or(reference_time);
                let end = start + chrono::Duration::days(1);
                (start, end)
            }
            ResetPeriod::Weekly => {
                let days_since_monday = reference_time.weekday().num_days_from_monday() as i64;
                let start = (reference_time.date_naive()
                    - chrono::Duration::days(days_since_monday))
                .and_hms_opt(0, 0, 0)
                .map(|naive| Utc.from_utc_datetime(&naive))
                .unwrap_or(reference_time);
                let end = start + chrono::Duration::weeks(1);
                (start, end)
            }
            ResetPeriod::Monthly => {
                let year = reference_time.year();
                let month = reference_time.month();

                let start = Utc
                    .with_ymd_and_hms(year, month, 1, 0, 0, 0)
                    .single()
                    .unwrap_or(reference_time);

                let end = if month == 12 {
                    Utc.with_ymd_and_hms(year + 1, 1, 1, 0, 0, 0).single()
                } else {
                    Utc.with_ymd_and_hms(year, month + 1, 1, 0, 0, 0).single()
                }
                .unwrap_or(start + chrono::Duration::days(31));

                (start, end)
            }
            ResetPeriod::Yearly => {
                let year = reference_time.year();
                let start = Utc
                    .with_ymd_and_hms(year, 1, 1, 0, 0, 0)
                    .single()
                    .unwrap_or(reference_time);
                let end = Utc
                    .with_ymd_and_hms(year + 1, 1, 1, 0, 0, 0)
                    .single()
                    .unwrap_or(start + chrono::Duration::days(365));
                (start, end)
            }
            ResetPeriod::Never => {
                // For "Never" reset period, use epoch as start and far future as end
                let start = DateTime::from_timestamp(0, 0).unwrap_or(reference_time);
                let end = DateTime::from_timestamp(i64::MAX / 1000, 0)
                    .unwrap_or(reference_time + chrono::Duration::days(365 * 100));
                (start, end)
            }
        }
    }

    /// Check if a given time is within the current period defined by this reset period
    pub fn is_current_period(
        &self,
        reference_time: DateTime<Utc>,
        check_time: DateTime<Utc>,
    ) -> bool {
        let (start, end) = self.period_bounds(reference_time);
        check_time >= start && check_time < end
    }
}

/// Configuration containing multiple quotas
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaConfig {
    pub quotas: std::collections::HashMap<ResetPeriod, u64>,
}

impl QuotaConfig {
    /// Create a new empty quota configuration
    pub fn new() -> Self {
        Self {
            quotas: std::collections::HashMap::new(),
        }
    }

    /// Create an empty quota configuration
    pub fn empty() -> Self {
        Self::new()
    }

    // TODO: Remove these convenience methods if not needed -------------------
    /// Create a configuration with daily quota only
    pub fn daily_only(limit: u64) -> Self {
        let mut quotas = std::collections::HashMap::new();
        quotas.insert(ResetPeriod::Daily, limit);
        Self { quotas }
    }

    /// Create a configuration with monthly quota only
    pub fn monthly_only(limit: u64) -> Self {
        let mut quotas = std::collections::HashMap::new();
        quotas.insert(ResetPeriod::Monthly, limit);
        Self { quotas }
    }

    /// Create a configuration with both daily and monthly quotas
    pub fn both(daily_limit: u64, monthly_limit: u64) -> Self {
        let mut quotas = std::collections::HashMap::new();
        quotas.insert(ResetPeriod::Daily, daily_limit);
        quotas.insert(ResetPeriod::Monthly, monthly_limit);
        Self { quotas }
    }
    // ------------------------------------------------------------------------

    /// Add a quota to this configuration
    pub fn with_quota(mut self, period: ResetPeriod, limit: u64) -> Self {
        self.quotas.insert(period, limit);
        self
    }

    /// Add a daily quota
    pub fn with_daily(self, limit: u64) -> Self {
        self.with_quota(ResetPeriod::Daily, limit)
    }

    /// Add a weekly quota
    pub fn with_weekly(self, limit: u64) -> Self {
        self.with_quota(ResetPeriod::Weekly, limit)
    }

    /// Add a monthly quota
    pub fn with_monthly(self, limit: u64) -> Self {
        self.with_quota(ResetPeriod::Monthly, limit)
    }

    /// Add a yearly quota
    pub fn with_yearly(self, limit: u64) -> Self {
        self.with_quota(ResetPeriod::Yearly, limit)
    }

    /// Add a permanent quota (never resets)
    pub fn with_permanent(self, limit: u64) -> Self {
        self.with_quota(ResetPeriod::Never, limit)
    }

    /// Check if any quotas are configured
    pub fn has_quotas(&self) -> bool {
        !self.quotas.is_empty()
    }

    /// Validate the quota configuration
    pub fn validate(&self) -> Result<()> {
        // Validate limits are positive
        for &limit in self.quotas.values() {
            if limit == 0 {
                return Err(RateLimitError::invalid_config(
                    "Quota limit must be greater than 0",
                ));
            }
        }
        Ok(())
    }

    /// Get quota limit by reset period
    pub fn get_limit(&self, period: &ResetPeriod) -> Option<u64> {
        self.quotas.get(period).copied()
    }

    /// Get all quotas as a reference to the HashMap
    pub fn get_quotas(&self) -> &std::collections::HashMap<ResetPeriod, u64> {
        &self.quotas
    }
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Usage tracking for a specific quota period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaUsage {
    /// Number of requests used in this period
    pub used: u64,
    /// The reset period for this quota
    pub reset_period: ResetPeriod,
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

    /// Add usage to this period
    pub fn add_usage(&mut self, count: u64) {
        self.used += count;
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

/// Complete quota tracker for a key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaTracker {
    /// Usage tracking for each configured quota
    pub usage: std::collections::HashMap<ResetPeriod, QuotaUsage>,
    /// The quota configuration
    pub config: QuotaConfig,
    /// Last time this tracker was updated
    pub last_updated: DateTime<Utc>,
}

impl QuotaTracker {
    /// Create a new quota tracker with the given configuration
    pub fn new(config: QuotaConfig) -> Result<Self> {
        config.validate()?;

        let now = Utc::now();
        let mut usage = std::collections::HashMap::new();

        for (&period, &limit) in &config.quotas {
            let quota_usage = QuotaUsage::new(period, limit, now);
            usage.insert(period, quota_usage);
        }

        Ok(Self {
            usage,
            config,
            last_updated: now,
        })
    }

    /// Check quota without consuming it
    pub fn check_quota(&mut self, count: u64) -> QuotaDecision {
        let now = Utc::now();
        self.refresh_periods(now);

        // Check all quotas
        for usage in self.usage.values() {
            if usage.used + count > usage.limit {
                return QuotaDecision::denied(
                    QuotaViolationType::Quota(usage.reset_period),
                    self.get_status(),
                    usage.used,
                    usage.limit,
                    usage.time_until_reset(now),
                    self.config.clone(),
                );
            }
        }

        QuotaDecision::allowed(self.get_status(), self.config.clone())
    }

    /// Consume quota (should only be called after a successful check)
    pub fn consume_quota(&mut self, count: u64) {
        let now = Utc::now();
        self.refresh_periods(now);

        for usage in self.usage.values_mut() {
            usage.add_usage(count);
        }

        self.last_updated = now;
    }

    /// Get current quota status
    pub fn get_status(&self) -> QuotaStatus {
        let mut period_statuses = std::collections::HashMap::new();

        for (period, usage) in &self.usage {
            let status = QuotaPeriodStatus {
                used: usage.used,
                limit: usage.limit,
                remaining: usage.remaining(),
                reset_time: usage.period_end,
            };
            period_statuses.insert(*period, status);
        }

        QuotaStatus {
            periods: period_statuses,
            config: self.config.clone(),
        }
    }

    /// Reset quota usage
    pub fn reset_quota(&mut self) {
        let now = Utc::now();
        for usage in self.usage.values_mut() {
            usage.reset(now);
        }
        self.last_updated = now;
    }

    /// Update quota configuration
    pub fn update_config(&mut self, new_config: QuotaConfig) -> Result<()> {
        new_config.validate()?;

        let now = Utc::now();
        let mut new_usage = std::collections::HashMap::new();

        // Preserve existing usage for quotas that still exist
        for (&period, &limit) in &new_config.quotas {
            if let Some(existing_usage) = self.usage.get(&period) {
                let mut updated_usage = existing_usage.clone();
                updated_usage.limit = limit;
                updated_usage.last_updated = now;
                new_usage.insert(period, updated_usage);
            } else {
                let quota_usage = QuotaUsage::new(period, limit, now);
                new_usage.insert(period, quota_usage);
            }
        }

        self.usage = new_usage;
        self.config = new_config;
        self.last_updated = now;
        Ok(())
    }

    /// Refresh quota periods (reset if expired)
    fn refresh_periods(&mut self, now: DateTime<Utc>) {
        for usage in self.usage.values_mut() {
            if usage.is_expired(now) {
                usage.reset(now);
            }
        }
        self.last_updated = now;
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
    /// The quota configuration
    pub config: QuotaConfig,
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
    /// The quota configuration used
    pub config: QuotaConfig,
}

impl QuotaDecision {
    /// Create an allowed decision
    pub fn allowed(status: QuotaStatus, config: QuotaConfig) -> Self {
        Self {
            allowed: true,
            violation_type: None,
            status,
            used: None,
            limit: None,
            retry_after: None,
            config,
        }
    }

    /// Create a denied decision
    pub fn denied(
        violation_type: QuotaViolationType,
        status: QuotaStatus,
        used: u64,
        limit: u64,
        retry_after: chrono::Duration,
        config: QuotaConfig,
    ) -> Self {
        Self {
            allowed: false,
            violation_type: Some(violation_type),
            status,
            used: Some(used),
            limit: Some(limit),
            retry_after: Some(retry_after),
            config,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reset_period_daily_bounds() {
        let reference = Utc.with_ymd_and_hms(2023, 6, 15, 14, 30, 0).unwrap();
        let (start, end) = ResetPeriod::Daily.period_bounds(reference);

        assert_eq!(start, Utc.with_ymd_and_hms(2023, 6, 15, 0, 0, 0).unwrap());
        assert_eq!(end, Utc.with_ymd_and_hms(2023, 6, 16, 0, 0, 0).unwrap());
    }

    #[test]
    fn test_reset_period_weekly_bounds() {
        // Thursday, June 15, 2023
        let reference = Utc.with_ymd_and_hms(2023, 6, 15, 14, 30, 0).unwrap();
        let (start, end) = ResetPeriod::Weekly.period_bounds(reference);

        // Should start on Monday, June 12, 2023
        assert_eq!(start, Utc.with_ymd_and_hms(2023, 6, 12, 0, 0, 0).unwrap());
        assert_eq!(end, Utc.with_ymd_and_hms(2023, 6, 19, 0, 0, 0).unwrap());
    }

    #[test]
    fn test_reset_period_monthly_bounds() {
        let reference = Utc.with_ymd_and_hms(2023, 6, 15, 14, 30, 0).unwrap();
        let (start, end) = ResetPeriod::Monthly.period_bounds(reference);

        assert_eq!(start, Utc.with_ymd_and_hms(2023, 6, 1, 0, 0, 0).unwrap());
        assert_eq!(end, Utc.with_ymd_and_hms(2023, 7, 1, 0, 0, 0).unwrap());
    }

    #[test]
    fn test_quota_config_creation() {
        let config = QuotaConfig::both(100, 1000);
        assert_eq!(config.quotas.len(), 2);
        assert!(config.has_quotas());

        let daily_limit = config.get_limit(&ResetPeriod::Daily).unwrap();
        assert_eq!(daily_limit, 100);

        let monthly_limit = config.get_limit(&ResetPeriod::Monthly).unwrap();
        assert_eq!(monthly_limit, 1000);
    }

    #[test]
    fn test_quota_config_validation() {
        let config = QuotaConfig::new().with_daily(100).with_monthly(1000);
        assert!(config.validate().is_ok());

        // Test zero limit
        let zero_config = QuotaConfig::new().with_daily(0);
        assert!(zero_config.validate().is_err());
    }

    #[test]
    fn test_quota_usage() {
        let now = Utc::now();
        let mut usage = QuotaUsage::new(ResetPeriod::Daily, 100, now);

        assert_eq!(usage.used, 0);
        assert_eq!(usage.remaining(), 100);
        assert!(!usage.is_exhausted());

        usage.add_usage(50);
        assert_eq!(usage.used, 50);
        assert_eq!(usage.remaining(), 50);
        assert!(!usage.is_exhausted());

        usage.add_usage(50);
        assert_eq!(usage.used, 100);
        assert_eq!(usage.remaining(), 0);
        assert!(usage.is_exhausted());
    }

    #[test]
    fn test_quota_tracker_basic() {
        let config = QuotaConfig::daily_only(100);
        let mut tracker = QuotaTracker::new(config).unwrap();

        let decision = tracker.check_quota(50);
        assert!(decision.allowed);

        tracker.consume_quota(50);
        let status = tracker.get_status();
        let daily_status = status.get_period_status(&ResetPeriod::Daily).unwrap();
        assert_eq!(daily_status.used, 50);
        assert_eq!(daily_status.remaining, 50);
    }

    #[test]
    fn test_quota_tracker_violation() {
        let config = QuotaConfig::daily_only(100);
        let mut tracker = QuotaTracker::new(config).unwrap();

        // Consume most quota
        tracker.consume_quota(90);

        // Should deny request that exceeds quota
        let decision = tracker.check_quota(20);
        assert!(!decision.allowed);
        assert!(decision.is_daily_violation());
        assert_eq!(decision.used, Some(90));
        assert_eq!(decision.limit, Some(100));
    }

    #[test]
    fn test_multiple_quotas() {
        let config = QuotaConfig::new()
            .with_daily(100)
            .with_weekly(500)
            .with_monthly(2000);
        let mut tracker = QuotaTracker::new(config).unwrap();

        // Should pass all quotas
        let decision = tracker.check_quota(50);
        assert!(decision.allowed);

        tracker.consume_quota(50);
        let status = tracker.get_status();

        assert_eq!(status.periods.len(), 3);
        assert!(status.get_period_status(&ResetPeriod::Daily).is_some());
        assert!(status.get_period_status(&ResetPeriod::Weekly).is_some());
        assert!(status.get_period_status(&ResetPeriod::Monthly).is_some());
    }

    #[test]
    fn test_quota_reset() {
        let config = QuotaConfig::daily_only(100);
        let mut tracker = QuotaTracker::new(config).unwrap();

        tracker.consume_quota(75);
        tracker.reset_quota();

        let status = tracker.get_status();
        let daily_status = status.get_period_status(&ResetPeriod::Daily).unwrap();
        assert_eq!(daily_status.used, 0);
        assert_eq!(daily_status.remaining, 100);
    }

    #[test]
    fn test_quota_status_methods() {
        let config = QuotaConfig::both(100, 1000);
        let mut tracker = QuotaTracker::new(config).unwrap();

        tracker.consume_quota(100); // Exhaust daily quota
        let status = tracker.get_status();

        assert!(status.is_any_exhausted());
        assert_eq!(status.max_utilization(), 1.0);

        let daily_status = status.get_period_status(&ResetPeriod::Daily).unwrap();
        assert!(daily_status.is_exhausted());
        assert_eq!(daily_status.utilization(), 1.0);

        let monthly_status = status.get_period_status(&ResetPeriod::Monthly).unwrap();
        assert!(!monthly_status.is_exhausted());
        assert_eq!(monthly_status.utilization(), 0.1);
    }

    #[test]
    fn test_permanent_quota() {
        let config = QuotaConfig::new().with_permanent(1000);
        let mut tracker = QuotaTracker::new(config).unwrap();

        // Consume some quota
        tracker.consume_quota(500);

        let decision = tracker.check_quota(400);
        assert!(decision.allowed);

        // Should deny when exceeding permanent quota
        let decision = tracker.check_quota(600);
        assert!(!decision.allowed);
        assert!(decision.is_permanent_violation());
    }
}
