use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::config::RateLimitConfig;
use crate::error::{RateLimitDecision, RateLimitError, Result};

/// Token bucket implementation for rate limiting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBucket {
    /// Number of available tokens
    tokens: f64,

    /// Last time the bucket was refilled
    #[serde(with = "instant_serde")]
    last_refill: Instant,

    /// Maximum number of tokens (burst size)
    max_tokens: f64,

    /// Rate at which tokens are refilled (tokens per millisecond)
    refill_rate: f64,

    /// Configuration used for this bucket
    config: RateLimitConfig,
}

impl TokenBucket {
    /// Create a new token bucket with the given configuration
    pub fn new(config: RateLimitConfig) -> Result<Self> {
        config
            .validate()
            .map_err(|msg| RateLimitError::invalid_config(&msg))?;

        let max_tokens = config.max_tokens();
        let refill_rate = config.refill_rate_per_ms();

        Ok(Self {
            tokens: max_tokens, // Start with full bucket
            last_refill: Instant::now(),
            max_tokens,
            refill_rate,
            config,
        })
    }

    /// Try to consume tokens from the bucket
    pub fn try_consume(&mut self, tokens: f64) -> RateLimitDecision {
        self.refill();

        if self.tokens >= tokens {
            self.tokens -= tokens;
            RateLimitDecision::allowed(self.tokens, self.max_tokens, self.config.clone())
        } else {
            let deficit = tokens - self.tokens;
            let retry_after_ms = (deficit / self.refill_rate).ceil() as u64;

            RateLimitDecision::denied(
                self.tokens,
                self.max_tokens,
                retry_after_ms,
                self.config.clone(),
            )
        }
    }

    /// Refill the bucket based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let elapsed_ms = elapsed.as_millis() as f64;

        if elapsed_ms > 0.0 {
            let tokens_to_add = elapsed_ms * self.refill_rate;
            self.tokens = (self.tokens + tokens_to_add).min(self.max_tokens);
            self.last_refill = now;
        }
    }

    /// Get the current number of available tokens
    pub fn available_tokens(&mut self) -> f64 {
        self.refill();
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

    /// Get the configuration used by this bucket
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Check if the bucket is empty
    pub fn is_empty(&mut self) -> bool {
        self.refill();
        self.tokens < 1.0
    }

    /// Check if the bucket is full
    pub fn is_full(&mut self) -> bool {
        self.refill();
        self.tokens >= self.max_tokens
    }

    /// Get the time since last refill
    pub fn time_since_last_refill(&self) -> Duration {
        Instant::now().duration_since(self.last_refill)
    }

    /// Reset the bucket to full capacity
    pub fn reset(&mut self) {
        self.tokens = self.max_tokens;
        self.last_refill = Instant::now();
    }

    /// Update the bucket configuration
    pub fn update_config(&mut self, new_config: RateLimitConfig) -> Result<()> {
        new_config
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

        // Update configuration
        self.config = new_config;
        self.max_tokens = self.config.max_tokens();
        self.refill_rate = self.config.refill_rate_per_ms();

        // Adjust current tokens based on new capacity
        self.tokens = (self.max_tokens * token_ratio).min(self.max_tokens);

        Ok(())
    }
}

/// Serde support for Instant serialization
mod instant_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(instant: &Instant, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Convert Instant to SystemTime for serialization
        let system_time = SystemTime::now()
            .checked_sub(instant.elapsed())
            .unwrap_or(UNIX_EPOCH);

        let duration_since_epoch = system_time
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);

        duration_since_epoch.as_nanos().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Instant, D::Error>
    where
        D: Deserializer<'de>,
    {
        let nanos = u128::deserialize(deserializer)?;
        let duration = Duration::from_nanos(nanos as u64);
        let system_time = UNIX_EPOCH + duration;

        // Convert back to Instant (approximate)
        let now_system = SystemTime::now();
        let now_instant = Instant::now();

        if let Ok(elapsed) = now_system.duration_since(system_time) {
            Ok(now_instant - elapsed)
        } else {
            Ok(now_instant)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_token_bucket_creation() {
        let config = RateLimitConfig::new(10, 20, 60);
        let bucket = TokenBucket::new(config).unwrap();

        assert_eq!(bucket.max_tokens(), 20.0);
        assert_eq!(bucket.refill_rate(), 0.01); // 10 per second = 0.01 per ms
    }

    #[test]
    fn test_token_consumption() {
        let config = RateLimitConfig::new(10, 20, 60);
        let mut bucket = TokenBucket::new(config).unwrap();

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
        let mut bucket = TokenBucket::new(config).unwrap();

        // Consume all tokens
        bucket.try_consume(2000.0);
        assert!(bucket.is_empty());

        // Wait a bit and check refill
        thread::sleep(Duration::from_millis(10));
        let tokens_before = bucket.available_tokens();

        thread::sleep(Duration::from_millis(10));
        let tokens_after = bucket.available_tokens();

        assert!(tokens_after > tokens_before);
    }

    #[test]
    fn test_bucket_state_checks() {
        let config = RateLimitConfig::new(10, 20, 60);
        let mut bucket = TokenBucket::new(config).unwrap();

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
        let mut bucket = TokenBucket::new(config).unwrap();

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
        let mut bucket = TokenBucket::new(config).unwrap();

        bucket.try_consume(15.0);
        assert_eq!(bucket.available_tokens(), 5.0);

        bucket.reset();
        assert_eq!(bucket.available_tokens(), 20.0);
        assert!(bucket.is_full());
    }

    #[test]
    fn test_invalid_config() {
        let invalid_config = RateLimitConfig::new(0, 20, 60);
        assert!(TokenBucket::new(invalid_config).is_err());

        let invalid_config = RateLimitConfig::new(20, 10, 60);
        assert!(TokenBucket::new(invalid_config).is_err());
    }

    #[test]
    fn test_serialization() {
        let config = RateLimitConfig::new(10, 20, 60);
        let bucket = TokenBucket::new(config).unwrap();

        let serialized = serde_json::to_string(&bucket).unwrap();
        let deserialized: TokenBucket = serde_json::from_str(&serialized).unwrap();

        assert_eq!(bucket.max_tokens(), deserialized.max_tokens());
        assert_eq!(bucket.refill_rate(), deserialized.refill_rate());
    }
}
