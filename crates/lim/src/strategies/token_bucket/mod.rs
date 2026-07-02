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
#[cfg(feature = "memory")]
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[cfg(any(feature = "memory", feature = "redis"))]
#[inline]
fn ceil_div_u64(value: u64, divisor: u64) -> u64 {
    value.div_ceil(divisor)
}

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
const TOKEN_SCALE: u64 = 1_000;

#[cfg(feature = "memory")]
const NO_REFILL_RETRY_AFTER_MS: u64 = u32::MAX as u64;

#[cfg(feature = "memory")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct TokenBucketState {
    tokens_scaled: u64,
    last_update_micros: u64,
}

#[cfg(feature = "memory")]
#[inline]
fn scale_tokens(tokens: u64) -> u64 {
    tokens.saturating_mul(TOKEN_SCALE)
}

#[cfg(feature = "memory")]
fn transition_state(
    state: TokenBucketState,
    now_micros: u64,
    capacity: u64,
    refill_rate: u64,
    cost: u64,
) -> (bool, TokenBucketState) {
    let capacity_scaled = scale_tokens(capacity);
    let cost_scaled = scale_tokens(cost);

    let elapsed_micros = now_micros.saturating_sub(state.last_update_micros);
    // Keep the same arithmetic as Redis:
    // tokens_to_add_scaled = elapsed_micros * refill_rate / 1000
    let tokens_to_add_scaled = if refill_rate > 0 {
        ((elapsed_micros as u128 * refill_rate as u128) / TOKEN_SCALE as u128).min(u64::MAX as u128)
            as u64
    } else {
        0
    };

    let available_scaled = state
        .tokens_scaled
        .saturating_add(tokens_to_add_scaled)
        .min(capacity_scaled);

    if available_scaled >= cost_scaled {
        (
            true,
            TokenBucketState {
                tokens_scaled: available_scaled - cost_scaled,
                last_update_micros: now_micros,
            },
        )
    } else {
        (
            false,
            TokenBucketState {
                tokens_scaled: available_scaled,
                last_update_micros: now_micros,
            },
        )
    }
}

#[cfg(feature = "memory")]
impl TokenBucket {
    pub fn new(store: Arc<State>, clock: Arc<CachedClock>, config: TokenBucketConfig) -> Self {
        // TTL should be long enough to cover the time to refill from empty
        // Plus some buffer for safety. 1 hour default if refill_rate is 0.
        let ttl = config
            .capacity
            .checked_div(config.refill_rate)
            .map_or(3600, |refill_secs| (refill_secs + 60).max(60));

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
        let now_micros = self.clock.now_micros();
        let capacity = self.capacity;
        let refill_rate = self.refill_rate;
        let ttl = self.ttl;

        let default_state = TokenBucketState {
            tokens_scaled: scale_tokens(capacity),
            last_update_micros: now_micros,
        };

        let (allowed, state) = self
            .store
            .measure_and_set(
                key,
                default_state,
                |stored| transition_state(stored, now_micros, capacity, refill_rate, cost),
                Some(ttl),
            )
            .await
            .map_err(|e| {
                crate::error::RateLimitError::MemoryError(format!(
                    "Failed to access in-memory store: {}",
                    e,
                ))
            })?;

        let remaining = state.tokens_scaled / TOKEN_SCALE;

        if allowed {
            let tokens_needed = capacity.saturating_sub(remaining);
            let reset_secs = if refill_rate > 0 && tokens_needed > 0 {
                ceil_div_u64(tokens_needed, refill_rate)
            } else {
                0
            };

            Ok(RateLimitDecision::allowed(
                capacity,
                remaining,
                Some(Duration::from_secs(reset_secs)),
            ))
        } else {
            let tokens_needed_scaled = scale_tokens(cost).saturating_sub(state.tokens_scaled);
            let retry_after_ms = if refill_rate > 0 {
                ceil_div_u64(tokens_needed_scaled, refill_rate)
            } else {
                NO_REFILL_RETRY_AFTER_MS
            };

            Ok(RateLimitDecision::denied(
                capacity,
                remaining,
                Some(Duration::from_millis(retry_after_ms)),
                None,
            ))
        }
    }
}

// =========================== REDIS IMPLEMENTATION ===========================

#[cfg(feature = "redis")]
static TOKEN_BUCKET_LUA_SCRIPT: std::sync::LazyLock<store::RedisScript> =
    std::sync::LazyLock::new(|| store::RedisScript::new(include_str!("token_bucket.lua")));

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
        // TTL should be long enough to cover the time to refill from empty.
        // 1 hour default if refill_rate is 0.
        let ttl = config
            .capacity
            .checked_div(config.refill_rate)
            .map_or(3600, |refill_secs| (refill_secs + 60).max(60));

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
        let mut con = self.store.get_connection();

        let script = &*TOKEN_BUCKET_LUA_SCRIPT;

        let capacity = self.capacity;
        let refill_rate = self.refill_rate;
        let ttl = self.ttl;

        let (allowed, remaining, retry_after_ms): (i32, u64, u64) = script
            .key(key)
            .arg(capacity)
            .arg(refill_rate)
            .arg(cost)
            .arg(ttl)
            .invoke_async(&mut con)
            .await
            .map_err(|e| {
                crate::error::RateLimitError::BackendError(format!("Redis script error: {}", e))
            })?;

        if allowed == 1 {
            // Calculate time until full
            let tokens_needed = capacity.saturating_sub(remaining);
            let reset_secs = if refill_rate > 0 && tokens_needed > 0 {
                ceil_div_u64(tokens_needed, refill_rate)
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

    #[cfg(feature = "memory")]
    #[test]
    fn test_memory_transition_keeps_fractional_refill_progress() {
        let state = TokenBucketState {
            tokens_scaled: 0,
            last_update_micros: 0,
        };

        // 50ms at 10 tokens/s -> 0.5 token (denied, but progress must be preserved)
        let (allowed_50ms, state_50ms) = transition_state(state, 50_000, 10, 10, 1);
        assert!(!allowed_50ms);
        assert_eq!(state_50ms.tokens_scaled, 500);

        // Another 50ms accumulates to a full token, so the request is now allowed.
        let (allowed_100ms, state_100ms) = transition_state(state_50ms, 100_000, 10, 10, 1);
        assert!(allowed_100ms);
        assert_eq!(state_100ms.tokens_scaled, 0);
    }

    #[cfg(feature = "memory")]
    #[test]
    fn test_memory_transition_supports_large_capacity_without_truncation() {
        let capacity = 2_000_000u64;
        let state = TokenBucketState {
            tokens_scaled: scale_tokens(capacity),
            last_update_micros: 1_000_000,
        };

        let (allowed, next) = transition_state(state, 1_000_000, capacity, 1, 1);
        assert!(allowed);
        assert_eq!(next.tokens_scaled, scale_tokens(capacity - 1));
    }

    #[cfg(feature = "memory")]
    #[test]
    fn test_memory_retry_after_ms_matches_redis_formula() {
        let refill_rate = 10u64;
        let available_scaled = 250u64;
        let cost_scaled = scale_tokens(1);
        let retry_after_ms =
            ceil_div_u64(cost_scaled.saturating_sub(available_scaled), refill_rate);

        assert_eq!(retry_after_ms, 75);
    }
}
