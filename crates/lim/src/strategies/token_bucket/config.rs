//! Token Bucket configuration
//!
//! This module provides configuration types for the token bucket rate limiter.

use serde::{Deserialize, Serialize};

/// Configuration for the Token Bucket rate limiter
///
/// # Example
///
/// ```
/// use lim::strategies::token_bucket::TokenBucketConfig;
///
/// // Allow 100 requests per second with burst up to 100
/// let config = TokenBucketConfig::per_second(100);
///
/// // Allow 1000 requests per minute with burst up to 200
/// let config = TokenBucketConfig::per_minute(1000).with_burst(200);
///
/// // Custom configuration: 50 capacity, 10 tokens/second refill
/// let config = TokenBucketConfig::new(50, 10);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenBucketConfig {
    /// Maximum number of tokens the bucket can hold (burst capacity)
    pub capacity: u64,

    /// Number of tokens added per second (sustained rate)
    pub refill_rate: u64,
}

impl Default for TokenBucketConfig {
    fn default() -> Self {
        Self {
            capacity: 100,
            refill_rate: 10,
        }
    }
}

impl TokenBucketConfig {
    /// Create a new token bucket configuration
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum tokens the bucket can hold
    /// * `refill_rate` - Tokens added per second
    pub fn new(capacity: u64, refill_rate: u64) -> Self {
        Self {
            capacity: capacity.max(1),
            refill_rate,
        }
    }

    /// Create a configuration for N requests per second
    ///
    /// Both capacity and refill rate are set to `count`,
    /// meaning the bucket starts full and refills at the same rate.
    pub fn per_second(count: u64) -> Self {
        let count = count.max(1);
        Self {
            capacity: count,
            refill_rate: count,
        }
    }

    /// Create a configuration for N requests per minute
    ///
    /// Capacity is set to `count`, refill rate is `count / 60` tokens per second.
    pub fn per_minute(count: u64) -> Self {
        let count = count.max(1);
        Self {
            capacity: count,
            refill_rate: (count / 60).max(1),
        }
    }

    /// Create a configuration for N requests per hour
    ///
    /// Capacity is set to `count`, refill rate is `count / 3600` tokens per second.
    pub fn per_hour(count: u64) -> Self {
        let count = count.max(1);
        Self {
            capacity: count,
            refill_rate: (count / 3600).max(1),
        }
    }

    /// Set a custom burst capacity (different from rate)
    ///
    /// This allows for temporary bursts above the sustained rate.
    ///
    /// # Example
    ///
    /// ```
    /// use lim::strategies::token_bucket::TokenBucketConfig;
    ///
    /// // 10 requests/second sustained, but allow bursts up to 100
    /// let config = TokenBucketConfig::per_second(10).with_burst(100);
    /// ```
    pub fn with_burst(mut self, burst: u64) -> Self {
        self.capacity = burst.max(1);
        self
    }

    /// Set a custom refill rate
    ///
    /// # Example
    ///
    /// ```
    /// use lim::strategies::token_bucket::TokenBucketConfig;
    ///
    /// // Bucket of 100 tokens, refilling at 5 tokens/second
    /// let config = TokenBucketConfig::new(100, 10).with_refill_rate(5);
    /// ```
    pub fn with_refill_rate(mut self, rate: u64) -> Self {
        self.refill_rate = rate;
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.capacity == 0 {
            return Err("capacity must be greater than 0");
        }
        Ok(())
    }

    /// Calculate how long it takes to refill from empty to full (in seconds)
    pub fn time_to_full(&self) -> u64 {
        if self.refill_rate == 0 {
            return u64::MAX;
        }
        (self.capacity + self.refill_rate - 1) / self.refill_rate // Ceiling division
    }

    /// Calculate how long until N tokens are available (in milliseconds)
    pub fn time_for_tokens_ms(&self, tokens: u64) -> u64 {
        if self.refill_rate == 0 {
            return u64::MAX;
        }
        // tokens / (refill_rate / 1000) = tokens * 1000 / refill_rate
        (tokens * 1000 + self.refill_rate - 1) / self.refill_rate // Ceiling division
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let config = TokenBucketConfig::default();
        assert_eq!(config.capacity, 100);
        assert_eq!(config.refill_rate, 10);
    }

    #[test]
    fn test_new() {
        let config = TokenBucketConfig::new(50, 5);
        assert_eq!(config.capacity, 50);
        assert_eq!(config.refill_rate, 5);
    }

    #[test]
    fn test_new_zero_capacity() {
        let config = TokenBucketConfig::new(0, 5);
        assert_eq!(config.capacity, 1); // Should be clamped to 1
    }

    #[test]
    fn test_per_second() {
        let config = TokenBucketConfig::per_second(100);
        assert_eq!(config.capacity, 100);
        assert_eq!(config.refill_rate, 100);
    }

    #[test]
    fn test_per_minute() {
        let config = TokenBucketConfig::per_minute(120);
        assert_eq!(config.capacity, 120);
        assert_eq!(config.refill_rate, 2);
    }

    #[test]
    fn test_per_hour() {
        let config = TokenBucketConfig::per_hour(3600);
        assert_eq!(config.capacity, 3600);
        assert_eq!(config.refill_rate, 1);
    }

    #[test]
    fn test_with_burst() {
        let config = TokenBucketConfig::per_second(10).with_burst(100);
        assert_eq!(config.capacity, 100);
        assert_eq!(config.refill_rate, 10);
    }

    #[test]
    fn test_with_refill_rate() {
        let config = TokenBucketConfig::new(100, 10).with_refill_rate(5);
        assert_eq!(config.capacity, 100);
        assert_eq!(config.refill_rate, 5);
    }

    #[test]
    fn test_validate() {
        let valid = TokenBucketConfig::new(100, 10);
        assert!(valid.validate().is_ok());
    }

    #[test]
    fn test_time_to_full() {
        let config = TokenBucketConfig::new(100, 10);
        assert_eq!(config.time_to_full(), 10);

        let config2 = TokenBucketConfig::new(100, 0);
        assert_eq!(config2.time_to_full(), u64::MAX);
    }

    #[test]
    fn test_time_for_tokens_ms() {
        let config = TokenBucketConfig::new(100, 10);
        // 5 tokens at 10/sec = 500ms
        assert_eq!(config.time_for_tokens_ms(5), 500);

        // 1 token at 10/sec = 100ms
        assert_eq!(config.time_for_tokens_ms(1), 100);
    }
}
