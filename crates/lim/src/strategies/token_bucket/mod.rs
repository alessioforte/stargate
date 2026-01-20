//! Token Bucket Rate Limiter
//!
//! The token bucket algorithm works by maintaining a bucket of tokens that
//! refills at a steady rate. Each request consumes tokens from the bucket.
//! If there aren't enough tokens, the request is denied.
//!
//! Key concepts:
//! - capacity: Maximum number of tokens the bucket can hold
//! - refill_rate: Number of tokens added per second
//! - tokens: Current number of available tokens
//!
//! ```text
//! Visualization:
//!
//!     Initial state (full bucket):
//!
//!         ┌─────────────────┐
//!         │ ○ ○ ○ ○ ○ ○ ○ ○ │  capacity = 8
//!         │ ○ ○ ○ ○ ○ ○ ○ ○ │  tokens = 8
//!         └─────────────────┘
//!
//!     After consuming 3 tokens:
//!
//!         ┌─────────────────┐
//!         │                 │
//!         │ ○ ○ ○ ○ ○       │  tokens = 5
//!         └─────────────────┘
//!              ↑
//!              refill_rate tokens/sec
//!
//!     Tokens refill over time until capacity is reached
//! ```

mod config;

pub use config::TokenBucketConfig;

use crate::clock::CachedClock;
use crate::decision::RateLimitDecision;
use crate::error::Result;
use crate::state::State;
use crate::strategies::RateLimit;
use async_trait::async_trait;
use core::time::Duration;
use std::sync::Arc;

// ========================== IN-MEMORY IMPLEMENTATION =========================

#[cfg(feature = "memory")]
pub struct TokenBucket {
    /// In-memory store
    store: Arc<State>,

    /// Clock used to measure time
    clock: Arc<CachedClock>,

    /// Maximum number of tokens the bucket can hold
    capacity: u64,

    /// Number of tokens added per second
    refill_rate: u64,

    /// TTL for stored state in seconds
    ttl: u64,
}

#[cfg(feature = "memory")]
impl TokenBucket {
    pub fn new(store: Arc<State>, clock: Arc<CachedClock>, config: TokenBucketConfig) -> Self {
        // TTL should be long enough to cover the time to refill from empty
        // Plus some buffer for safety
        let ttl = if config.refill_rate > 0 {
            ((config.capacity / config.refill_rate) + 60).max(60)
        } else {
            3600 // 1 hour default if refill_rate is 0
        };

        Self {
            store,
            clock,
            capacity: config.capacity,
            refill_rate: config.refill_rate,
            ttl,
        }
    }
}

#[cfg(feature = "memory")]
#[async_trait]
impl RateLimit for TokenBucket {
    async fn check(&self, key: &str, cost: u64) -> Result<RateLimitDecision> {
        use store::AtomicStore;

        let now_micros = self.clock.now_micros();
        let capacity = self.capacity;
        let refill_rate = self.refill_rate;
        let ttl = self.ttl;

        // We store: (tokens * 1_000_000) << 44 | last_update_micros
        // This packs both values into a single i64
        // tokens are stored as fixed-point with 6 decimal places (microtokens)
        // last_update is microseconds since epoch (fits in ~44 bits until year 2527)
        let tokens_key = format!("{}:tokens", key);
        let time_key = format!("{}:time", key);

        // Get current state
        let stored_tokens = self.store.get_i64(&tokens_key).await.ok().flatten();
        let stored_time = self.store.get_i64(&time_key).await.ok().flatten();

        let (current_tokens, last_update) = match (stored_tokens, stored_time) {
            (Some(t), Some(time)) => (t as u64, time as u64),
            _ => (capacity, now_micros), // Initialize with full bucket
        };

        // Calculate tokens to add based on elapsed time
        let elapsed_micros = now_micros.saturating_sub(last_update);
        let elapsed_secs_frac = elapsed_micros as f64 / 1_000_000.0;
        let tokens_to_add = (elapsed_secs_frac * refill_rate as f64) as u64;

        // Calculate new token count (capped at capacity)
        let available_tokens = current_tokens.saturating_add(tokens_to_add).min(capacity);

        // Check if we have enough tokens
        if available_tokens >= cost {
            let new_tokens = available_tokens - cost;

            // Update state
            let _ = self
                .store
                .set_i64(&tokens_key, new_tokens as i64, Some(ttl))
                .await;
            let _ = self
                .store
                .set_i64(&time_key, now_micros as i64, Some(ttl))
                .await;

            // Calculate time until bucket is full again
            let tokens_needed = capacity - new_tokens;
            let reset_secs = if refill_rate > 0 && tokens_needed > 0 {
                (tokens_needed as f64 / refill_rate as f64).ceil() as u64
            } else {
                0
            };

            Ok(RateLimitDecision::allowed(
                capacity,
                new_tokens,
                Some(Duration::from_secs(reset_secs)),
            ))
        } else {
            // Not enough tokens - calculate retry after
            let tokens_needed = cost - available_tokens;
            let retry_secs = if refill_rate > 0 {
                (tokens_needed as f64 / refill_rate as f64).ceil() as u64
            } else {
                u64::MAX // Never if no refill
            };

            // Update the time even on denial to keep state fresh
            let _ = self
                .store
                .set_i64(&tokens_key, available_tokens as i64, Some(ttl))
                .await;
            let _ = self
                .store
                .set_i64(&time_key, now_micros as i64, Some(ttl))
                .await;

            Ok(RateLimitDecision::denied(
                capacity,
                available_tokens,
                Some(Duration::from_secs(retry_secs)),
                None,
            ))
        }
    }
}

// =========================== REDIS IMPLEMENTATION ===========================

#[cfg(feature = "redis")]
pub struct TokenBucket {
    /// Redis store
    store: Arc<State>,

    /// Maximum number of tokens the bucket can hold
    capacity: u64,

    /// Number of tokens added per second
    refill_rate: u64,

    /// TTL for stored state in seconds
    ttl: u64,
}

#[cfg(feature = "redis")]
impl TokenBucket {
    pub fn new(store: Arc<State>, _clock: Arc<CachedClock>, config: TokenBucketConfig) -> Self {
        // TTL should be long enough to cover the time to refill from empty
        let ttl = if config.refill_rate > 0 {
            ((config.capacity / config.refill_rate) + 60).max(60)
        } else {
            3600
        };

        Self {
            store,
            capacity: config.capacity,
            refill_rate: config.refill_rate,
            ttl,
        }
    }
}

#[cfg(feature = "redis")]
#[async_trait]
impl RateLimit for TokenBucket {
    async fn check(&self, key: &str, cost: u64) -> Result<RateLimitDecision> {
        let mut con = self.store.get_connection().await.map_err(|e| {
            crate::error::RateLimitError::BackendError(format!(
                "Failed to get Redis connection: {}",
                e
            ))
        })?;

        let script = store::RedisScript::new(include_str!("token_bucket.lua"));

        let capacity = self.capacity;
        let refill_rate = self.refill_rate;
        let ttl = self.ttl;

        let (allowed, remaining, retry_after_ms): (i32, u64, u64) = script
            .key(key)
            .arg(capacity)
            .arg(refill_rate)
            .arg(cost)
            .arg(ttl)
            .invoke(&mut con)
            .map_err(|e| {
                crate::error::RateLimitError::BackendError(format!("Redis script error: {}", e))
            })?;

        if allowed == 1 {
            // Calculate time until full
            let tokens_needed = capacity.saturating_sub(remaining);
            let reset_secs = if refill_rate > 0 && tokens_needed > 0 {
                (tokens_needed as f64 / refill_rate as f64).ceil() as u64
            } else {
                0
            };

            Ok(RateLimitDecision::allowed(
                capacity,
                remaining,
                Some(Duration::from_secs(reset_secs)),
            ))
        } else {
            let retry_after = Duration::from_millis(retry_after_ms);
            Ok(RateLimitDecision::denied(
                capacity,
                remaining,
                Some(retry_after),
                None,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let config = TokenBucketConfig::new(100, 10);
        assert_eq!(config.capacity, 100);
        assert_eq!(config.refill_rate, 10);
    }

    #[test]
    fn test_config_per_second() {
        let config = TokenBucketConfig::per_second(50);
        assert_eq!(config.capacity, 50);
        assert_eq!(config.refill_rate, 50);
    }

    #[test]
    fn test_config_per_minute() {
        let config = TokenBucketConfig::per_minute(120);
        assert_eq!(config.capacity, 120);
        assert_eq!(config.refill_rate, 2); // 120 per minute = 2 per second
    }

    #[test]
    fn test_config_with_burst() {
        let config = TokenBucketConfig::per_second(10).with_burst(100);
        assert_eq!(config.capacity, 100);
        assert_eq!(config.refill_rate, 10);
    }

    #[test]
    fn test_refill_calculation() {
        // If we have 10 tokens/second refill rate
        // After 500ms we should have 5 tokens added
        let refill_rate = 10u64;
        let elapsed_micros = 500_000u64; // 500ms

        let elapsed_secs_frac = elapsed_micros as f64 / 1_000_000.0;
        let tokens_to_add = (elapsed_secs_frac * refill_rate as f64) as u64;

        assert_eq!(tokens_to_add, 5);
    }
}
