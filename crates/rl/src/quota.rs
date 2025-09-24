use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{RateLimitError, Result};

/// Quota configuration for daily and monthly limits
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaConfig {
    /// Daily quota limit (optional)
    pub daily_limit: Option<u64>,

    /// Monthly quota limit (optional)
    pub monthly_limit: Option<u64>,
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            daily_limit: None,
            monthly_limit: None,
        }
    }
}

impl QuotaConfig {
    /// Create a new quota configuration
    pub fn new(daily_limit: Option<u64>, monthly_limit: Option<u64>) -> Self {
        Self {
            daily_limit,
            monthly_limit,
        }
    }

    /// Create a quota configuration with only daily limit
    pub fn daily_only(limit: u64) -> Self {
        Self {
            daily_limit: Some(limit),
            monthly_limit: None,
        }
    }

    /// Create a quota configuration with only monthly limit
    pub fn monthly_only(limit: u64) -> Self {
        Self {
            daily_limit: None,
            monthly_limit: Some(limit),
        }
    }

    /// Create a quota configuration with both daily and monthly limits
    pub fn both(daily_limit: u64, monthly_limit: u64) -> Self {
        Self {
            daily_limit: Some(daily_limit),
            monthly_limit: Some(monthly_limit),
        }
    }

    /// Check if any quotas are configured
    pub fn has_quotas(&self) -> bool {
        self.daily_limit.is_some() || self.monthly_limit.is_some()
    }

    /// Validate the quota configuration
    pub fn validate(&self) -> Result<()> {
        if let (Some(daily), Some(monthly)) = (self.daily_limit, self.monthly_limit) {
            // Generally, monthly should be >= daily * 30, but we'll just warn if it's less than daily
            if monthly < daily {
                return Err(RateLimitError::invalid_config(
                    "Monthly quota should not be less than daily quota",
                ));
            }
        }
        Ok(())
    }
}

/// Quota usage tracking for a specific time period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaUsage {
    /// Number of requests used in this period
    pub used: u64,

    /// Timestamp when this period started
    pub period_start: DateTime<Utc>,

    /// Timestamp when this period ends
    pub period_end: DateTime<Utc>,
}

impl QuotaUsage {
    /// Create a new quota usage tracker for a daily period
    pub fn new_daily(start_date: DateTime<Utc>) -> Self {
        let period_start = start_date
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|naive| Utc.from_utc_datetime(&naive))
            .unwrap_or(start_date);
        let period_end = period_start + chrono::Duration::days(1);

        Self {
            used: 0,
            period_start,
            period_end,
        }
    }

    /// Create a new quota usage tracker for a monthly period
    pub fn new_monthly(start_date: DateTime<Utc>) -> Self {
        let year = start_date.year();
        let month = start_date.month();

        let period_start = Utc
            .with_ymd_and_hms(year, month, 1, 0, 0, 0)
            .single()
            .unwrap_or(start_date);

        let period_end = if month == 12 {
            Utc.with_ymd_and_hms(year + 1, 1, 1, 0, 0, 0).single()
        } else {
            Utc.with_ymd_and_hms(year, month + 1, 1, 0, 0, 0).single()
        }
        .unwrap_or(period_start + chrono::Duration::days(31));

        Self {
            used: 0,
            period_start,
            period_end,
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
    }

    /// Reset usage for this period
    pub fn reset(&mut self) {
        self.used = 0;
    }

    /// Get remaining time in this period
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
    /// Daily quota usage tracking
    pub daily_usage: Option<QuotaUsage>,

    /// Monthly quota usage tracking
    pub monthly_usage: Option<QuotaUsage>,

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
        let daily_usage = if config.daily_limit.is_some() {
            Some(QuotaUsage::new_daily(now))
        } else {
            None
        };

        let monthly_usage = if config.monthly_limit.is_some() {
            Some(QuotaUsage::new_monthly(now))
        } else {
            None
        };

        Ok(Self {
            daily_usage,
            monthly_usage,
            config,
            last_updated: now,
        })
    }

    /// Check if quota allows the given number of requests
    pub fn check_quota(&mut self, count: u64) -> QuotaDecision {
        let now = Utc::now();
        self.refresh_periods(now);

        // Check daily quota
        if let (Some(daily_usage), Some(daily_limit)) = (&self.daily_usage, self.config.daily_limit)
        {
            if daily_usage.used + count > daily_limit {
                return QuotaDecision::denied(
                    QuotaViolationType::Daily,
                    daily_usage.used,
                    daily_limit,
                    daily_usage.time_until_reset(now),
                    self.config.clone(),
                );
            }
        }

        // Check monthly quota
        if let (Some(monthly_usage), Some(monthly_limit)) =
            (&self.monthly_usage, self.config.monthly_limit)
        {
            if monthly_usage.used + count > monthly_limit {
                return QuotaDecision::denied(
                    QuotaViolationType::Monthly,
                    monthly_usage.used,
                    monthly_limit,
                    monthly_usage.time_until_reset(now),
                    self.config.clone(),
                );
            }
        }

        QuotaDecision::allowed(self.get_status(), self.config.clone())
    }

    /// Consume quota (after a successful quota check)
    pub fn consume_quota(&mut self, count: u64) {
        let now = Utc::now();
        self.refresh_periods(now);

        if let Some(daily_usage) = &mut self.daily_usage {
            daily_usage.add_usage(count);
        }

        if let Some(monthly_usage) = &mut self.monthly_usage {
            monthly_usage.add_usage(count);
        }

        self.last_updated = now;
    }

    /// Get current quota status
    pub fn get_status(&mut self) -> QuotaStatus {
        let now = Utc::now();
        self.refresh_periods(now);

        let daily_status = if let (Some(daily_usage), Some(daily_limit)) =
            (&self.daily_usage, self.config.daily_limit)
        {
            Some(QuotaPeriodStatus {
                used: daily_usage.used,
                limit: daily_limit,
                remaining: daily_limit.saturating_sub(daily_usage.used),
                reset_time: daily_usage.period_end,
            })
        } else {
            None
        };

        let monthly_status = if let (Some(monthly_usage), Some(monthly_limit)) =
            (&self.monthly_usage, self.config.monthly_limit)
        {
            Some(QuotaPeriodStatus {
                used: monthly_usage.used,
                limit: monthly_limit,
                remaining: monthly_limit.saturating_sub(monthly_usage.used),
                reset_time: monthly_usage.period_end,
            })
        } else {
            None
        };

        QuotaStatus {
            daily: daily_status,
            monthly: monthly_status,
            config: self.config.clone(),
        }
    }

    /// Reset quota usage (admin function)
    pub fn reset_quota(&mut self) {
        if let Some(daily_usage) = &mut self.daily_usage {
            daily_usage.reset();
        }
        if let Some(monthly_usage) = &mut self.monthly_usage {
            monthly_usage.reset();
        }
        self.last_updated = Utc::now();
    }

    /// Update the quota configuration
    pub fn update_config(&mut self, new_config: QuotaConfig) -> Result<()> {
        new_config.validate()?;

        let now = Utc::now();

        // Handle daily quota changes
        match (self.config.daily_limit, new_config.daily_limit) {
            (None, Some(_)) => {
                // Adding daily quota
                self.daily_usage = Some(QuotaUsage::new_daily(now));
            }
            (Some(_), None) => {
                // Removing daily quota
                self.daily_usage = None;
            }
            _ => {
                // Keep existing daily usage if it exists
            }
        }

        // Handle monthly quota changes
        match (self.config.monthly_limit, new_config.monthly_limit) {
            (None, Some(_)) => {
                // Adding monthly quota
                self.monthly_usage = Some(QuotaUsage::new_monthly(now));
            }
            (Some(_), None) => {
                // Removing monthly quota
                self.monthly_usage = None;
            }
            _ => {
                // Keep existing monthly usage if it exists
            }
        }

        self.config = new_config;
        self.last_updated = now;
        Ok(())
    }

    /// Refresh usage periods if they have expired
    fn refresh_periods(&mut self, now: DateTime<Utc>) {
        // Refresh daily usage if expired
        if let Some(daily_usage) = &mut self.daily_usage {
            if daily_usage.is_expired(now) {
                *daily_usage = QuotaUsage::new_daily(now);
            }
        }

        // Refresh monthly usage if expired
        if let Some(monthly_usage) = &mut self.monthly_usage {
            if monthly_usage.is_expired(now) {
                *monthly_usage = QuotaUsage::new_monthly(now);
            }
        }
    }
}

/// Type of quota violation
#[derive(Debug, Clone, PartialEq)]
pub enum QuotaViolationType {
    Daily,
    Monthly,
}

/// Status of a quota period (daily or monthly)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaPeriodStatus {
    /// Number of requests used in this period
    pub used: u64,

    /// Total limit for this period
    pub limit: u64,

    /// Remaining requests in this period
    pub remaining: u64,

    /// When this period resets
    pub reset_time: DateTime<Utc>,
}

impl QuotaPeriodStatus {
    /// Get utilization as a percentage (0.0 to 1.0)
    pub fn utilization(&self) -> f64 {
        if self.limit == 0 {
            0.0
        } else {
            self.used as f64 / self.limit as f64
        }
    }

    /// Check if quota is exhausted
    pub fn is_exhausted(&self) -> bool {
        self.remaining == 0
    }

    /// Get time until reset as std::time::Duration
    pub fn time_until_reset(&self) -> std::time::Duration {
        let now = Utc::now();
        if now >= self.reset_time {
            std::time::Duration::ZERO
        } else {
            (self.reset_time - now)
                .to_std()
                .unwrap_or(std::time::Duration::ZERO)
        }
    }
}

/// Complete quota status for a key
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaStatus {
    /// Daily quota status (if configured)
    pub daily: Option<QuotaPeriodStatus>,

    /// Monthly quota status (if configured)
    pub monthly: Option<QuotaPeriodStatus>,

    /// The quota configuration
    pub config: QuotaConfig,
}

impl QuotaStatus {
    /// Check if any quota is exhausted
    pub fn is_any_exhausted(&self) -> bool {
        self.daily.as_ref().map_or(false, |d| d.is_exhausted())
            || self.monthly.as_ref().map_or(false, |m| m.is_exhausted())
    }

    /// Get the most restrictive reset time (soonest)
    pub fn next_reset_time(&self) -> Option<DateTime<Utc>> {
        let daily_reset = self.daily.as_ref().map(|d| d.reset_time);
        let monthly_reset = self.monthly.as_ref().map(|m| m.reset_time);

        match (daily_reset, monthly_reset) {
            (Some(daily), Some(monthly)) => Some(daily.min(monthly)),
            (Some(daily), None) => Some(daily),
            (None, Some(monthly)) => Some(monthly),
            (None, None) => None,
        }
    }

    /// Get overall utilization (highest of daily/monthly)
    pub fn max_utilization(&self) -> f64 {
        let daily_util = self.daily.as_ref().map_or(0.0, |d| d.utilization());
        let monthly_util = self.monthly.as_ref().map_or(0.0, |m| m.utilization());
        daily_util.max(monthly_util)
    }
}

/// Quota checking decision
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaDecision {
    /// Whether the request is allowed
    pub allowed: bool,

    /// Type of quota violation (if denied)
    pub violation_type: Option<QuotaViolationType>,

    /// Current quota status
    pub status: Option<QuotaStatus>,

    /// Used requests for the violated quota period
    pub used: Option<u64>,

    /// Limit for the violated quota period
    pub limit: Option<u64>,

    /// Time until the violated quota resets
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
            status: Some(status),
            used: None,
            limit: None,
            retry_after: None,
            config,
        }
    }

    /// Create a denied decision
    pub fn denied(
        violation_type: QuotaViolationType,
        used: u64,
        limit: u64,
        retry_after: chrono::Duration,
        config: QuotaConfig,
    ) -> Self {
        Self {
            allowed: false,
            violation_type: Some(violation_type),
            status: None,
            used: Some(used),
            limit: Some(limit),
            retry_after: Some(retry_after),
            config,
        }
    }

    /// Get retry after as std::time::Duration
    pub fn retry_after_duration(&self) -> Option<std::time::Duration> {
        self.retry_after.and_then(|d| d.to_std().ok())
    }

    /// Check if this was a daily quota violation
    pub fn is_daily_violation(&self) -> bool {
        matches!(self.violation_type, Some(QuotaViolationType::Daily))
    }

    /// Check if this was a monthly quota violation
    pub fn is_monthly_violation(&self) -> bool {
        matches!(self.violation_type, Some(QuotaViolationType::Monthly))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    #[test]
    fn test_quota_config_creation() {
        let config = QuotaConfig::new(Some(1000), Some(30000));
        assert_eq!(config.daily_limit, Some(1000));
        assert_eq!(config.monthly_limit, Some(30000));
        assert!(config.has_quotas());

        let daily_only = QuotaConfig::daily_only(500);
        assert_eq!(daily_only.daily_limit, Some(500));
        assert_eq!(daily_only.monthly_limit, None);

        let monthly_only = QuotaConfig::monthly_only(10000);
        assert_eq!(monthly_only.daily_limit, None);
        assert_eq!(monthly_only.monthly_limit, Some(10000));
    }

    #[test]
    fn test_quota_config_validation() {
        let valid_config = QuotaConfig::both(1000, 30000);
        assert!(valid_config.validate().is_ok());

        let invalid_config = QuotaConfig::both(1000, 500);
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_quota_usage_daily() {
        let start = Utc
            .with_ymd_and_hms(2024, 1, 15, 14, 30, 0)
            .single()
            .unwrap();
        let usage = QuotaUsage::new_daily(start);

        // Should start at midnight of the same day
        assert_eq!(usage.period_start.hour(), 0);
        assert_eq!(usage.period_start.minute(), 0);
        assert_eq!(usage.period_start.second(), 0);
        assert_eq!(usage.period_start.day(), 15);

        // Should end at midnight of next day
        assert_eq!(usage.period_end.day(), 16);
        assert_eq!(usage.period_end.hour(), 0);
    }

    #[test]
    fn test_quota_usage_monthly() {
        let start = Utc
            .with_ymd_and_hms(2024, 6, 15, 14, 30, 0)
            .single()
            .unwrap();
        let usage = QuotaUsage::new_monthly(start);

        // Should start at beginning of month
        assert_eq!(usage.period_start.day(), 1);
        assert_eq!(usage.period_start.hour(), 0);
        assert_eq!(usage.period_start.month(), 6);

        // Should end at beginning of next month
        assert_eq!(usage.period_end.day(), 1);
        assert_eq!(usage.period_end.month(), 7);
    }

    #[test]
    fn test_quota_tracker_basic() {
        let config = QuotaConfig::both(100, 3000);
        let mut tracker = QuotaTracker::new(config).unwrap();

        // Should allow requests within quota
        let decision = tracker.check_quota(10);
        assert!(decision.allowed);

        // Consume the quota
        tracker.consume_quota(10);

        // Check status
        let status = tracker.get_status();
        assert_eq!(status.daily.unwrap().used, 10);
        assert_eq!(status.monthly.unwrap().used, 10);
    }

    #[test]
    fn test_quota_tracker_daily_violation() {
        let config = QuotaConfig::daily_only(100);
        let mut tracker = QuotaTracker::new(config).unwrap();

        // Consume most of the quota
        tracker.consume_quota(95);

        // Should deny request that would exceed quota
        let decision = tracker.check_quota(10);
        assert!(!decision.allowed);
        assert!(decision.is_daily_violation());
        assert_eq!(decision.used, Some(95));
        assert_eq!(decision.limit, Some(100));
    }

    #[test]
    fn test_quota_tracker_monthly_violation() {
        let config = QuotaConfig::monthly_only(1000);
        let mut tracker = QuotaTracker::new(config).unwrap();

        // Consume most of the quota
        tracker.consume_quota(995);

        // Should deny request that would exceed quota
        let decision = tracker.check_quota(10);
        assert!(!decision.allowed);
        assert!(decision.is_monthly_violation());
        assert_eq!(decision.used, Some(995));
        assert_eq!(decision.limit, Some(1000));
    }

    #[test]
    fn test_quota_reset() {
        let config = QuotaConfig::both(100, 3000);
        let mut tracker = QuotaTracker::new(config).unwrap();

        tracker.consume_quota(50);
        assert_eq!(tracker.get_status().daily.unwrap().used, 50);

        tracker.reset_quota();
        assert_eq!(tracker.get_status().daily.unwrap().used, 0);
        assert_eq!(tracker.get_status().monthly.unwrap().used, 0);
    }

    #[test]
    fn test_quota_status_methods() {
        let daily_status = QuotaPeriodStatus {
            used: 75,
            limit: 100,
            remaining: 25,
            reset_time: Utc::now() + chrono::Duration::hours(6),
        };

        assert_eq!(daily_status.utilization(), 0.75);
        assert!(!daily_status.is_exhausted());

        let exhausted_status = QuotaPeriodStatus {
            used: 100,
            limit: 100,
            remaining: 0,
            reset_time: Utc::now() + chrono::Duration::hours(6),
        };

        assert!(exhausted_status.is_exhausted());
        assert_eq!(exhausted_status.utilization(), 1.0);
    }
}
