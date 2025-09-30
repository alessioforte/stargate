use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{RateLimitError, Result};

/// Reset period for quota limits
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ResetPeriod {
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Never,
}

impl std::str::FromStr for ResetPeriod {
    type Err = RateLimitError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "daily" => Ok(ResetPeriod::Daily),
            "weekly" => Ok(ResetPeriod::Weekly),
            "monthly" => Ok(ResetPeriod::Monthly),
            "yearly" => Ok(ResetPeriod::Yearly),
            "never" => Ok(ResetPeriod::Never),
            _ => Err(RateLimitError::invalid_config("Invalid reset period")),
        }
    }
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
        let config = QuotaConfig::new().with_daily(100).with_monthly(1000);
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
}
