use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::config::{RateLimitConfig, ResetPeriod};
use crate::decision::RateLimitDecision;
use crate::decision::{QuotaDecision, QuotaPeriodStatus, QuotaStatus, QuotaViolationType};
use crate::error::{RateLimitError, Result};
use crate::usage::QuotaUsage;

/// Token bucket implementation for rate limiting
///
/// # Thread Safety
/// This struct is NOT thread-safe. If used in multi-threaded environments,
/// external synchronization (e.g., Mutex, RwLock) is required.
///
/// # Time Handling
/// Uses UTC wall clock time consistently to avoid issues with system clock changes
/// and to ensure proper serialization across process boundaries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bucket {
    /// Number of available tokens
    tokens: f64,

    /// Last time the bucket was refilled
    last_refill: DateTime<Utc>,

    /// Maximum number of tokens (burst size)
    max_tokens: f64,

    /// Rate at which tokens are refilled (tokens per millisecond)
    refill_rate: f64,

    /// Usage tracking for each configured quota
    usage: std::collections::HashMap<ResetPeriod, QuotaUsage>,

    /// Last time this tracker was updated
    last_updated: DateTime<Utc>,
}

impl Bucket {
    /// Create a new token bucket with the given configuration
    pub fn new(config: RateLimitConfig) -> Result<Self> {
        config
            .validate()
            .map_err(|msg| RateLimitError::invalid_config(&msg))?;

        let max_tokens = config.max_tokens();
        let refill_rate = config.refill_rate_per_ms();

        let now = Utc::now();
        let mut usage = std::collections::HashMap::new();

        if let Some(quotas) = config.quotas {
            for (&period, &limit) in &quotas {
                let quota_usage = QuotaUsage::new(period, limit, now);
                usage.insert(period, quota_usage);
            }
        }

        Ok(Self {
            tokens: max_tokens, // Start with full bucket
            last_refill: now,
            max_tokens,
            refill_rate,
            usage,
            last_updated: now,
        })
    }

    /// Try to consume tokens from the bucket
    pub fn try_consume(&mut self, tokens: f64) -> RateLimitDecision {
        let now = Utc::now();
        self.try_consume_with_time(tokens, now)
    }

    /// Try to consume tokens with a specific timestamp
    pub fn try_consume_with_time(&mut self, tokens: f64, now: DateTime<Utc>) -> RateLimitDecision {
        self.refill_with_time(now);

        if self.tokens >= tokens {
            self.tokens -= tokens;
            RateLimitDecision::allowed(self.tokens, self.max_tokens)
        } else {
            let deficit = tokens - self.tokens;
            let retry_after_ms = (deficit / self.refill_rate).ceil() as u64;

            RateLimitDecision::denied(self.tokens, self.max_tokens, retry_after_ms)
        }
    }

    /// Refill the bucket based on elapsed time
    ///
    /// Returns true if tokens were actually added, false otherwise.
    /// Handles system clock changes gracefully by treating negative elapsed time as zero.
    fn refill(&mut self) -> bool {
        let now = Utc::now();
        self.refill_with_time(now)
    }

    /// Refill the bucket with a specific timestamp to avoid multiple time calls
    fn refill_with_time(&mut self, now: DateTime<Utc>) -> bool {
        // Early return if bucket is already full
        if self.tokens >= self.max_tokens {
            return false;
        }

        let elapsed_ms = (now - self.last_refill).num_milliseconds();

        // Early return if no time has passed or clock moved backwards
        if elapsed_ms <= 0 {
            // If clock moved backwards significantly, update last_refill to prevent issues
            if elapsed_ms < -1000 {
                // More than 1 second backwards
                self.last_refill = now;
            }
            return false;
        }

        let tokens_to_add = elapsed_ms as f64 * self.refill_rate;

        // Only update if we're actually adding tokens
        if tokens_to_add > 0.0 {
            self.tokens = (self.tokens + tokens_to_add).min(self.max_tokens);
            self.last_refill = now;
            true
        } else {
            false
        }
    }

    /// Get the current number of available tokens
    pub fn available_tokens(&mut self) -> f64 {
        let now = Utc::now();
        self.available_tokens_with_time(now)
    }

    /// Get available tokens with a specific timestamp
    pub fn available_tokens_with_time(&mut self, now: DateTime<Utc>) -> f64 {
        self.refill_with_time(now);
        self.tokens
    }

    /// Get the maximum tokens (burst size)
    pub fn max_tokens(&self) -> f64 {
        self.max_tokens
    }

    /// Get the refill rate in tokens per millisecond
    pub fn refill_rate(&self) -> f64 {
        self.refill_rate
    }

    /// Check if the bucket is empty
    pub fn is_empty(&mut self) -> bool {
        let now = Utc::now();
        self.is_empty_with_time(now)
    }

    /// Check if the bucket is empty with a specific timestamp
    pub fn is_empty_with_time(&mut self, now: DateTime<Utc>) -> bool {
        self.refill_with_time(now);
        self.tokens < 1.0
    }

    /// Check if the bucket is full
    pub fn is_full(&mut self) -> bool {
        let now = Utc::now();
        self.is_full_with_time(now)
    }

    /// Check if the bucket is full with a specific timestamp
    pub fn is_full_with_time(&mut self, now: DateTime<Utc>) -> bool {
        // Optimize: if already full, no need to refill
        if self.tokens >= self.max_tokens {
            return true;
        }
        self.refill_with_time(now);
        self.tokens >= self.max_tokens
    }

    /// Get the time since last refill
    ///
    /// Returns a Duration representing elapsed time since last refill.
    /// If system clock moved backwards, returns Duration::ZERO for safety.
    pub fn time_since_last_refill(&self) -> Duration {
        let chrono_duration = Utc::now() - self.last_refill;
        let ms = chrono_duration.num_milliseconds();

        // Handle clock moving backwards gracefully
        if ms < 0 {
            Duration::ZERO
        } else {
            Duration::from_millis(ms as u64)
        }
    }

    /// Reset the bucket to full capacity
    pub fn reset(&mut self) {
        let now = Utc::now();
        self.tokens = self.max_tokens;
        self.last_refill = now;
    }

    /// Update the bucket configuration
    pub fn update_config(&mut self, config: RateLimitConfig) -> Result<()> {
        config
            .validate()
            .map_err(|msg| RateLimitError::invalid_config(&msg))?;

        // Refill with current config before updating
        self.refill();

        // Calculate the ratio of tokens to preserve
        let token_ratio = if self.max_tokens > 0.0 {
            self.tokens / self.max_tokens
        } else {
            1.0
        };

        self.max_tokens = config.max_tokens();
        self.refill_rate = config.refill_rate_per_ms();

        // Adjust current tokens based on new capacity
        self.tokens = (self.max_tokens * token_ratio).min(self.max_tokens);

        let now = Utc::now();

        if let Some(quotas) = config.quotas {
            // Optimize: if quotas are identical, skip the expensive update
            if self.usage.len() == quotas.len()
                && quotas.iter().all(|(&period, &limit)| {
                    self.usage
                        .get(&period)
                        .map_or(false, |usage| usage.limit == limit)
                })
            {
                // Quotas are identical, just update timestamps
                for usage in self.usage.values_mut() {
                    usage.last_updated = now;
                }
            } else {
                // Update existing quotas in place, remove obsolete ones, add new ones
                let mut new_usage = std::collections::HashMap::with_capacity(quotas.len());

                for (&period, &limit) in &quotas {
                    if let Some(mut existing_usage) = self.usage.remove(&period) {
                        // Reuse existing usage without cloning
                        existing_usage.limit = limit;
                        existing_usage.last_updated = now;
                        new_usage.insert(period, existing_usage);
                    } else {
                        // Create new quota usage
                        let quota_usage = QuotaUsage::new(period, limit, now);
                        new_usage.insert(period, quota_usage);
                    }
                }

                self.usage = new_usage;
            }
        } else {
            // No quotas in new config, clear all
            self.usage.clear();
        }
        self.last_updated = now;

        Ok(())
    }

    /// Check quota without consuming it
    pub fn check_quota(&mut self, count: u64) -> QuotaDecision {
        let now = Utc::now();
        self.check_quota_with_time(count, now)
    }

    /// Check quota with a specific timestamp to avoid multiple time calls
    pub fn check_quota_with_time(&mut self, count: u64, now: DateTime<Utc>) -> QuotaDecision {
        // Early exit if no quotas configured
        if self.usage.is_empty() {
            return QuotaDecision::allowed(QuotaStatus {
                periods: std::collections::HashMap::new(),
            });
        }

        self.refresh_periods(now);

        // Single pass through quotas: check for violations and build status
        let mut period_statuses = std::collections::HashMap::with_capacity(self.usage.len());
        let mut violation: Option<(&ResetPeriod, &QuotaUsage)> = None;

        for (period, usage) in &self.usage {
            // Create status for this period
            let status = QuotaPeriodStatus {
                used: usage.used,
                limit: usage.limit,
                remaining: usage.remaining(),
                reset_time: usage.period_end,
            };
            period_statuses.insert(*period, status);

            // Check for violation (only record first one found)
            if violation.is_none() {
                let new_usage = usage.used.saturating_add(count);
                if new_usage > usage.limit {
                    violation = Some((period, usage));
                }
            }
        }

        let quota_status = QuotaStatus {
            periods: period_statuses,
        };

        if let Some((period, usage)) = violation {
            QuotaDecision::denied(
                QuotaViolationType::Quota(*period),
                quota_status,
                usage.used,
                usage.limit,
                usage.time_until_reset(now),
            )
        } else {
            QuotaDecision::allowed(quota_status)
        }
    }

    /// Consume quota (should only be called after a successful check)
    pub fn consume_quota(&mut self, count: u64) {
        let now = Utc::now();
        self.consume_quota_with_time(count, now);
    }

    /// Consume quota with a specific timestamp
    pub fn consume_quota_with_time(&mut self, count: u64, now: DateTime<Utc>) {
        self.refresh_periods(now);

        for usage in self.usage.values_mut() {
            usage.add_usage(count);
        }

        self.last_updated = now;
    }

    /// Check and consume quota in a single operation
    pub fn check_and_consume_quota(&mut self, count: u64) -> QuotaDecision {
        let now = Utc::now();
        self.check_and_consume_quota_with_time(count, now)
    }

    /// Check and consume quota with a single timestamp for consistency
    pub fn check_and_consume_quota_with_time(
        &mut self,
        count: u64,
        now: DateTime<Utc>,
    ) -> QuotaDecision {
        let decision = self.check_quota_with_time(count, now);
        if decision.allowed {
            self.consume_quota_with_time(count, now);
        }
        decision
    }

    /// Get current quota status
    pub fn get_quota_status(&self) -> QuotaStatus {
        let now = Utc::now();
        self.get_quota_status_with_time(now)
    }

    /// Get quota status with a specific timestamp
    pub fn get_quota_status_with_time(&self, _now: DateTime<Utc>) -> QuotaStatus {
        let mut period_statuses = std::collections::HashMap::with_capacity(self.usage.len());

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
        }
    }

    /// Reset quota usage
    pub fn reset_quota(&mut self) {
        let now = Utc::now();
        self.reset_quota_with_time(now);
    }

    /// Reset quota with a specific timestamp
    pub fn reset_quota_with_time(&mut self, now: DateTime<Utc>) {
        for usage in self.usage.values_mut() {
            usage.reset(now);
        }
        self.last_updated = now;
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

    /// Combined rate limit and quota check with single timestamp for optimal performance
    pub fn check_rate_limit_and_quota(
        &mut self,
        tokens: f64,
        quota_count: u64,
    ) -> (RateLimitDecision, QuotaDecision) {
        let now = Utc::now();

        // Early exit if quotas are empty - only do rate limiting
        if self.usage.is_empty() {
            let rate_decision = self.try_consume_with_time(tokens, now);
            let quota_decision = QuotaDecision::allowed(QuotaStatus {
                periods: std::collections::HashMap::new(),
            });
            return (rate_decision, quota_decision);
        }

        let rate_decision = self.try_consume_with_time(tokens, now);
        let quota_decision = self.check_and_consume_quota_with_time(quota_count, now);

        (rate_decision, quota_decision)
    }

    /// Check if bucket has any quotas configured
    pub fn has_quotas(&self) -> bool {
        !self.usage.is_empty()
    }

    /// Get the number of configured quota periods
    pub fn quota_count(&self) -> usize {
        self.usage.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_token_bucket_creation() {
        let config = RateLimitConfig::new(10, 20, 60);
        let bucket = Bucket::new(config).unwrap();

        assert_eq!(bucket.max_tokens(), 20.0);
        assert_eq!(bucket.refill_rate(), 0.01); // 10 per second = 0.01 per ms
    }

    #[test]
    fn test_token_consumption() {
        let config = RateLimitConfig::new(10, 20, 60);
        let mut bucket = Bucket::new(config).unwrap();

        // Should allow consuming tokens when bucket is full
        let decision = bucket.try_consume(5.0);
        assert!(decision.allowed);
        assert_eq!(decision.remaining_tokens, 15.0);

        // Should deny when trying to consume more than available
        let decision = bucket.try_consume(20.0);
        assert!(!decision.allowed);
        assert!(decision.retry_after_ms.is_some());
    }

    #[test]
    fn test_token_refill() {
        let config = RateLimitConfig::new(1000, 2000, 60); // High rate for testing
        let mut bucket = Bucket::new(config).unwrap();

        // Consume all tokens
        bucket.try_consume(2000.0);
        assert!(bucket.is_empty());

        // Test refill after 10ms (should add 10 tokens at 1 token/ms)
        let now = Utc::now();
        bucket.last_refill = now - chrono::Duration::milliseconds(10);
        let tokens_after_10ms = bucket.available_tokens();
        assert_eq!(tokens_after_10ms, 10.0);

        // Reset and test 20ms refill
        bucket.try_consume(10.0); // Consume the tokens we just got
        bucket.last_refill = now - chrono::Duration::milliseconds(20);
        let tokens_after_20ms = bucket.available_tokens();
        assert_eq!(tokens_after_20ms, 20.0);
    }

    #[test]
    fn test_bucket_state_checks() {
        let config = RateLimitConfig::new(10, 20, 60);
        let mut bucket = Bucket::new(config).unwrap();

        assert!(bucket.is_full());
        assert!(!bucket.is_empty());

        bucket.try_consume(20.0);
        assert!(!bucket.is_full());

        bucket.try_consume(1.0); // This should fail but doesn't consume
        assert!(bucket.is_empty());
    }

    #[test]
    fn test_config_update() {
        let config = RateLimitConfig::new(10, 20, 60);
        let mut bucket = Bucket::new(config).unwrap();

        // Consume half the tokens
        bucket.try_consume(10.0);
        assert_eq!(bucket.available_tokens(), 10.0);

        // Update to double capacity
        let new_config = RateLimitConfig::new(20, 40, 60);
        bucket.update_config(new_config).unwrap();

        // Should maintain the same ratio of tokens
        assert_eq!(bucket.max_tokens(), 40.0);
        assert_eq!(bucket.available_tokens(), 20.0);
    }

    #[test]
    fn test_bucket_reset() {
        let config = RateLimitConfig::new(10, 20, 60);
        let mut bucket = Bucket::new(config).unwrap();

        bucket.try_consume(15.0);
        assert_eq!(bucket.available_tokens(), 5.0);

        bucket.reset();
        assert_eq!(bucket.available_tokens(), 20.0);
        assert!(bucket.is_full());
    }

    #[test]
    fn test_invalid_config() {
        let invalid_config = RateLimitConfig::new(0, 20, 60);
        assert!(Bucket::new(invalid_config).is_err());

        let invalid_config = RateLimitConfig::new(20, 10, 60);
        assert!(Bucket::new(invalid_config).is_err());
    }

    #[test]
    fn test_serialization() {
        let config = RateLimitConfig::new(10, 20, 60);
        let bucket = Bucket::new(config).unwrap();

        let serialized = serde_json::to_string(&bucket).unwrap();
        let deserialized: Bucket = serde_json::from_str(&serialized).unwrap();

        assert_eq!(bucket.max_tokens(), deserialized.max_tokens());
        assert_eq!(bucket.refill_rate(), deserialized.refill_rate());

        // Test that DateTime serialization works properly
        assert!(serialized.contains("last_refill"));
        assert!(serialized.contains("last_updated"));
    }

    #[test]
    fn test_time_consistency_methods() {
        let config = RateLimitConfig::new(100, 200, 60);
        let mut bucket = Bucket::new(config).unwrap();

        let now = Utc::now();

        // Test that methods with timestamp produce consistent results
        bucket.try_consume(50.0);

        let tokens_1 = bucket.available_tokens_with_time(now);
        let tokens_2 = bucket.available_tokens_with_time(now);
        assert_eq!(
            tokens_1, tokens_2,
            "Same timestamp should produce same results"
        );

        // Test that different methods use same timestamp consistently
        let is_empty_1 = bucket.is_empty_with_time(now);
        let is_full_1 = bucket.is_full_with_time(now);
        let tokens_3 = bucket.available_tokens_with_time(now);

        // These should all be consistent with each other
        assert_eq!(is_empty_1, tokens_3 < 1.0);
        assert_eq!(is_full_1, tokens_3 >= bucket.max_tokens());
    }

    #[test]
    fn test_quota_overflow_protection() {
        use crate::config::ResetPeriod;
        let mut quotas = std::collections::HashMap::new();
        quotas.insert(ResetPeriod::Daily, 100);

        let config = RateLimitConfig::new(1000, 2000, 60).with_quotas(quotas);
        let mut bucket = Bucket::new(config).unwrap();

        let now = Utc::now();

        // Manually set usage near overflow limit
        if let Some(usage) = bucket.usage.get_mut(&ResetPeriod::Daily) {
            usage.used = u64::MAX - 50;
        }

        // This should not panic due to overflow
        let decision = bucket.check_quota_with_time(100, now);
        assert!(!decision.allowed, "Should deny due to quota violation");
    }

    #[test]
    fn test_combined_rate_limit_and_quota() {
        use crate::config::ResetPeriod;
        let mut quotas = std::collections::HashMap::new();
        quotas.insert(ResetPeriod::Daily, 5);

        let config = RateLimitConfig::new(10, 20, 60).with_quotas(quotas);
        let mut bucket = Bucket::new(config).unwrap();

        // Test combined operation
        let (rate_decision, quota_decision) = bucket.check_rate_limit_and_quota(1.0, 3);

        assert!(rate_decision.allowed, "Rate limit should allow");
        assert!(quota_decision.allowed, "Quota should allow");

        // Test when quota would be exceeded
        let (rate_decision2, quota_decision2) = bucket.check_rate_limit_and_quota(1.0, 10);

        assert!(rate_decision2.allowed, "Rate limit should still allow");
        assert!(!quota_decision2.allowed, "Quota should deny");
    }

    #[test]
    fn test_early_exit_optimizations() {
        let config = RateLimitConfig::new(10, 20, 60); // No quotas
        let mut bucket = Bucket::new(config).unwrap();

        // Test early exit when no quotas
        let decision = bucket.check_quota(100);
        assert!(decision.allowed, "Should allow when no quotas configured");
        assert!(
            decision.status.periods.is_empty(),
            "Should have empty periods"
        );

        // Test combined method early exit
        let (rate_decision, quota_decision) = bucket.check_rate_limit_and_quota(1.0, 100);
        assert!(rate_decision.allowed, "Rate limit should allow");
        assert!(quota_decision.allowed, "Should allow when no quotas");
        assert!(quota_decision.status.periods.is_empty());
    }

    #[test]
    fn test_config_update_optimization() {
        use crate::config::ResetPeriod;
        let mut quotas = std::collections::HashMap::new();
        quotas.insert(ResetPeriod::Daily, 100);
        quotas.insert(ResetPeriod::Monthly, 1000);

        let config = RateLimitConfig::new(10, 20, 60).with_quotas(quotas.clone());
        let mut bucket = Bucket::new(config).unwrap();

        // Consume some quota
        bucket.consume_quota(50);

        // Update with identical quotas (should be optimized)
        let config2 = RateLimitConfig::new(15, 25, 60).with_quotas(quotas);
        bucket.update_config(config2).unwrap();

        // Usage should be preserved
        let status = bucket.get_quota_status();
        let daily_status = status.get_period_status(&ResetPeriod::Daily).unwrap();
        assert_eq!(daily_status.used, 50, "Usage should be preserved");
    }

    #[test]
    fn test_single_pass_quota_check() {
        use crate::config::ResetPeriod;
        let mut quotas = std::collections::HashMap::new();
        quotas.insert(ResetPeriod::Daily, 100);
        quotas.insert(ResetPeriod::Monthly, 1000);
        quotas.insert(ResetPeriod::Weekly, 500);

        let config = RateLimitConfig::new(10, 20, 60).with_quotas(quotas);
        let mut bucket = Bucket::new(config).unwrap();

        let now = Utc::now();

        // Set one quota near limit
        if let Some(usage) = bucket.usage.get_mut(&ResetPeriod::Daily) {
            usage.used = 95;
        }

        // This should fail on daily quota and return status for all periods
        let decision = bucket.check_quota_with_time(10, now);
        assert!(!decision.allowed, "Should deny due to daily quota");
        assert_eq!(
            decision.status.periods.len(),
            3,
            "Should have status for all periods"
        );

        // Verify all periods have status
        assert!(
            decision
                .status
                .get_period_status(&ResetPeriod::Daily)
                .is_some()
        );
        assert!(
            decision
                .status
                .get_period_status(&ResetPeriod::Weekly)
                .is_some()
        );
        assert!(
            decision
                .status
                .get_period_status(&ResetPeriod::Monthly)
                .is_some()
        );
    }

    #[test]
    fn test_utility_methods() {
        use crate::config::ResetPeriod;
        let mut quotas = std::collections::HashMap::new();
        quotas.insert(ResetPeriod::Daily, 100);

        let config = RateLimitConfig::new(10, 20, 60).with_quotas(quotas);
        let bucket = Bucket::new(config).unwrap();

        assert!(bucket.has_quotas(), "Should have quotas");
        assert_eq!(bucket.quota_count(), 1, "Should have 1 quota");

        let config_no_quotas = RateLimitConfig::new(10, 20, 60);
        let bucket_no_quotas = Bucket::new(config_no_quotas).unwrap();

        assert!(!bucket_no_quotas.has_quotas(), "Should not have quotas");
        assert_eq!(bucket_no_quotas.quota_count(), 0, "Should have 0 quotas");
    }

    #[test]
    fn test_refill_performance_optimization() {
        let config = RateLimitConfig::new(100, 200, 60);
        let mut bucket = Bucket::new(config).unwrap();

        // Full bucket should return false for refill (no work needed)
        assert!(!bucket.refill());

        // Consume some tokens
        bucket.try_consume(50.0);

        // No time passed, should return false
        assert!(!bucket.refill());

        // Simulate time passing
        bucket.last_refill = Utc::now() - chrono::Duration::milliseconds(10);

        // Now refill should return true (work was done)
        assert!(bucket.refill());
    }

    #[test]
    fn test_performance_single_timestamp() {
        let config = RateLimitConfig::new(1000, 2000, 60);
        let mut bucket = Bucket::new(config).unwrap();

        let now = Utc::now();

        // Multiple operations with same timestamp should be consistent
        bucket.try_consume_with_time(100.0, now);
        let tokens_1 = bucket.available_tokens_with_time(now);
        let tokens_2 = bucket.available_tokens_with_time(now);
        let is_empty = bucket.is_empty_with_time(now);
        let is_full = bucket.is_full_with_time(now);

        assert_eq!(
            tokens_1, tokens_2,
            "Multiple calls with same timestamp should be identical"
        );
        assert_eq!(is_empty, tokens_1 < 1.0);
        assert_eq!(is_full, tokens_1 >= bucket.max_tokens());
    }
}
