// GCRA (Generic Cell Rate Algorithm)
// ================================================================
// Key concepts:
// - TAT (Theoretical Arrival Time): when the next token will be available
// - τ (tau): time between tokens
// - T (burst): maximum burst capacity
//
// Initial timeline (empty bucket):
//
//       t0
//        │
//        ▼
// ─────────────────────────────────────────────────────────────────▶
//                                                                 time
// TAT = t0
//
//
// Allowed request:
//
//                      allow_at (past)           t0        TAT
//                             │                   │         │
//                             ▼                   ▼         ▼
// ──────────────────────────────────────────────────────────+──────▶
//                             │<------ burst time (T) ----->│     time
//                             └-----------------------------┘
//
//
// Denied request:
//
//                 t0   allow_at (past)                     TAT
//                  │          │                             │
//                  ▼          ▼                             ▼
// ──────────────────────────────────────────────────────────+──────▶
//                             │<------ burst time (T) ----->│     time
//                             └-----------------------------┘
//

mod quota;

use crate::clock::CachedClock;
use crate::decision::RateLimitDecision;
use crate::error::Result;
use crate::state::State;
use crate::strategies::RateLimit;
use async_trait::async_trait;
use core::cmp;
use core::time::Duration;
use std::sync::Arc;

pub use quota::Quota;

// ========================== IN-MEMORY IMPLEMENTATION =========================

#[cfg(feature = "memory")]
pub struct Gcra {
    /// in-memory store
    store: Arc<State>,

    /// clock used to measure time
    clock: Arc<CachedClock>,

    /// tau is the time it takes to replenish one token in microseconds
    tau: u64,

    /// delay variation tolerance in microseconds
    burst: u64,

    /// ttl is the time to live for the token bucket in seconds
    ttl: u64,
}

#[cfg(feature = "memory")]
impl Gcra {
    pub fn new(store: Arc<State>, clock: Arc<CachedClock>, quota: Quota) -> Self {
        let tau = cmp::max(quota.replenish_1_per, Duration::from_micros(1)).as_micros() as u64;
        let burst = tau * (quota.max_burst.get() - 1) as u64;
        let ttl = ((tau * quota.max_burst.get() as u64) / 1_000_000).max(60);
        Gcra {
            store,
            clock,
            tau,
            burst,
            ttl,
        }
    }
}

#[cfg(feature = "memory")]
#[async_trait]
impl RateLimit for Gcra {
    fn kind(&self) -> crate::strategies::LimitKind {
        crate::strategies::LimitKind::Rate
    }

    async fn check(&self, key: &str, _cost: u64) -> Result<RateLimitDecision> {
        let t0 = self.clock.now_micros();

        let tau = self.tau;
        let burst = self.burst;
        let ttl = self.ttl;

        let (allow, tat) = self
            .store
            .measure_and_set_i64(
                key,
                |stored_tat| {
                    // Initialize to t0 if fresh (stored = 0)
                    let tat = if stored_tat == 0 {
                        t0
                    } else {
                        stored_tat as u64
                    };

                    let allowed_at = tat.saturating_sub(burst);
                    if t0 < allowed_at {
                        // Reject - return unchanged TAT
                        (false, tat as i64)
                    } else {
                        // Accept - advance TAT
                        let next = cmp::max(tat, t0) + tau;
                        (true, next as i64)
                    }
                },
                Some(ttl),
            )
            .await
            .map_err(|e| {
                crate::error::RateLimitError::MemoryError(format!(
                    "Failed to access in-memory store: {}",
                    e,
                ))
            })?;

        let tat = tat as u64;

        // Rate limit (requests per second)
        let limit = 1_000_000u64.checked_div(tau).unwrap_or(0).max(1);

        // Remaining capacity calculation (FIXED)
        let total_capacity = burst + tau; // = tau * max_burst
        let used_capacity = tat.saturating_sub(t0);
        let remaining = total_capacity
            .saturating_sub(used_capacity)
            .checked_div(tau)
            .unwrap_or(0);

        if allow {
            Ok(RateLimitDecision::allowed(limit, remaining, None))
        } else {
            // Retry after: ceil to milliseconds for consistent units across all strategies
            let wait_micros = tat.saturating_sub(burst).saturating_sub(t0);
            let wait_millis = wait_micros.div_ceil(1_000);
            let retry_after = Duration::from_millis(wait_millis);
            Ok(RateLimitDecision::denied(
                limit,
                remaining,
                Some(retry_after),
                None,
            ))
        }
    }
}

// =========================== REDIS IMPLEMENTATION ===========================

#[cfg(feature = "redis")]
static GCRA_LUA_SCRIPT: std::sync::LazyLock<store::RedisScript> =
    std::sync::LazyLock::new(|| store::RedisScript::new(include_str!("gcra.lua")));

#[cfg(feature = "redis")]
pub struct Gcra {
    /// Redis store
    store: Arc<State>,

    /// tau is the time it takes to replenish one token in microseconds
    tau: u64,

    /// burst is the maximum number of tokens that can be consumed in one burst
    burst: u64,

    /// ttl is the time to live for the token bucket in seconds
    ttl: u64,
}

#[cfg(feature = "redis")]
impl Gcra {
    pub fn new(store: Arc<State>, _clock: Arc<CachedClock>, quota: Quota) -> Self {
        // tau is the time it takes to replenish one token
        let tau = cmp::max(quota.replenish_1_per, Duration::from_micros(1)).as_micros() as u64;
        // burst is the maximum number of tokens that can be consumed in one burst
        let burst = tau * (quota.max_burst.get() - 1) as u64;
        // ttl is the time to live for the token bucket
        let ttl = ((tau * quota.max_burst.get() as u64) / 1_000_000).max(60);
        Gcra {
            store,
            tau,
            burst,
            ttl,
        }
    }
}

#[cfg(feature = "redis")]
#[async_trait]
impl RateLimit for Gcra {
    fn kind(&self) -> crate::strategies::LimitKind {
        crate::strategies::LimitKind::Rate
    }

    async fn check(&self, key: &str, _cost: u64) -> Result<RateLimitDecision> {
        let mut con = self.store.get_connection();

        let script = &*GCRA_LUA_SCRIPT;

        let tau = self.tau;
        let burst = self.burst;
        let ttl = self.ttl;

        let (allow, retry_after, remaining_burst): (i32, u64, u64) = script
            .key(key)
            .arg(tau)
            .arg(burst)
            .arg(ttl)
            .invoke_async(&mut con)
            .await
            .map_err(|e| {
                crate::error::RateLimitError::BackendError(format!("Redis script error: {}", e))
            })?;

        // Calculate rate limit (requests per second)
        // tau is microseconds per request, so requests per second = 1_000_000 / tau
        let limit = 1_000_000u64.checked_div(tau).unwrap_or(0).max(1);

        // Convert remaining burst capacity (microseconds) to number of requests
        let remaining = remaining_burst.checked_div(tau).unwrap_or(0);

        if allow == 1 {
            Ok(RateLimitDecision::allowed(limit, remaining, None))
        } else {
            // Ceil microseconds to milliseconds for consistent units across all strategies
            let wait_millis = retry_after / 1_000 + u64::from(retry_after % 1_000 != 0);
            Ok(RateLimitDecision::denied(
                limit,
                remaining,
                Some(Duration::from_millis(wait_millis)),
                None,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::num::NonZeroU32;

    // ========================== QUOTA TESTS ==========================

    #[test]
    fn test_quota_default() {
        let quota = Quota::default();
        assert_eq!(quota.max_burst.get(), 2);
        assert_eq!(quota.replenish_1_per, Duration::from_secs(5));
    }

    #[test]
    fn test_quota_per_second() {
        let quota = Quota::per_second(10);
        assert_eq!(quota.max_burst.get(), 10);
        // 10 requests per second = 1 request per 100ms
        assert_eq!(quota.replenish_1_per, Duration::from_millis(100));
    }

    #[test]
    fn test_quota_per_minute() {
        let quota = Quota::per_minute(60);
        assert_eq!(quota.max_burst.get(), 60);
        // 60 requests per minute = 1 request per second
        assert_eq!(quota.replenish_1_per, Duration::from_secs(1));
    }

    #[test]
    fn test_quota_per_hour() {
        let quota = Quota::per_hour(3600);
        assert_eq!(quota.max_burst.get(), 3600);
        // 3600 requests per hour = 1 request per second
        assert_eq!(quota.replenish_1_per, Duration::from_secs(1));
    }

    #[test]
    fn test_quota_per_duration() {
        // 100 requests per 10 seconds = 10 req/s
        let quota = Quota::per_duration(100, Duration::from_secs(10));
        assert_eq!(quota.max_burst.get(), 100);
        // 100ms per request
        assert_eq!(quota.replenish_1_per, Duration::from_millis(100));
    }

    #[test]
    fn test_quota_with_burst() {
        let quota = Quota::per_second(10).with_burst(50);
        assert_eq!(quota.max_burst.get(), 50);
        // replenish rate unchanged
        assert_eq!(quota.replenish_1_per, Duration::from_millis(100));
    }

    #[test]
    fn test_quota_with_burst_zero_clamped() {
        let quota = Quota::per_second(10).with_burst(0);
        // Should be clamped to 1
        assert_eq!(quota.max_burst.get(), 1);
    }

    #[test]
    fn test_quota_per_second_zero_clamped() {
        let quota = Quota::per_second(0);
        // Should be clamped to 1
        assert_eq!(quota.max_burst.get(), 1);
    }

    #[test]
    fn test_quota_validate_success() {
        let quota = Quota::per_second(10);
        assert!(quota.validate().is_ok());
    }

    #[test]
    fn test_quota_validate_zero_replenish() {
        let quota = Quota {
            max_burst: NonZeroU32::new(10).unwrap(),
            replenish_1_per: Duration::ZERO,
        };
        assert!(quota.validate().is_err());
        assert_eq!(
            quota.validate().unwrap_err(),
            "replenish_1_per cannot be zero"
        );
    }

    #[test]
    fn test_quota_clone() {
        let quota1 = Quota::per_second(10);
        let quota2 = quota1;
        assert_eq!(quota1, quota2);
    }

    #[test]
    fn test_quota_equality() {
        let quota1 = Quota::per_second(10);
        let quota2 = Quota::per_second(10);
        let quota3 = Quota::per_second(20);

        assert_eq!(quota1, quota2);
        assert_ne!(quota1, quota3);
    }

    // ========================== GCRA ALGORITHM TESTS ==========================

    #[test]
    fn test_tau_calculation() {
        // For 10 requests per second, tau should be 100ms = 100,000 microseconds
        let quota = Quota::per_second(10);
        let tau = cmp::max(quota.replenish_1_per, Duration::from_micros(1)).as_micros() as u64;
        assert_eq!(tau, 100_000);
    }

    #[test]
    fn test_burst_calculation() {
        // For 10 requests per second with max_burst of 10
        // burst = tau * (max_burst - 1) = 100,000 * 9 = 900,000 microseconds
        let quota = Quota::per_second(10);
        let tau = cmp::max(quota.replenish_1_per, Duration::from_micros(1)).as_micros() as u64;
        let burst = tau * (quota.max_burst.get() - 1) as u64;
        assert_eq!(burst, 900_000);
    }

    #[test]
    fn test_ttl_calculation() {
        // TTL should be at least 60 seconds
        let quota = Quota::per_second(10);
        let tau = cmp::max(quota.replenish_1_per, Duration::from_micros(1)).as_micros() as u64;
        let ttl = ((tau * quota.max_burst.get() as u64) / 1_000_000).max(60);
        // tau * max_burst = 100,000 * 10 = 1,000,000 microseconds = 1 second
        // max(1, 60) = 60
        assert_eq!(ttl, 60);
    }

    #[test]
    fn test_ttl_calculation_large_quota() {
        // For a quota that takes longer to refill
        let quota = Quota::per_minute(1); // 1 request per minute
        let tau = cmp::max(quota.replenish_1_per, Duration::from_micros(1)).as_micros() as u64;
        let ttl = ((tau * quota.max_burst.get() as u64) / 1_000_000).max(60);
        // tau = 60 seconds = 60,000,000 microseconds
        // ttl = 60,000,000 / 1,000,000 = 60 seconds
        assert_eq!(ttl, 60);
    }

    #[test]
    fn test_limit_calculation() {
        // For tau = 100,000 microseconds (100ms)
        // limit = 1,000,000 / 100,000 = 10 requests per second
        let tau = 100_000u64;
        let limit = 1_000_000u64.checked_div(tau).unwrap_or(0).max(1);
        assert_eq!(limit, 10);
    }

    #[test]
    fn test_limit_calculation_small_tau() {
        // For tau = 1 microsecond (very fast)
        // limit = 1,000,000 / 1 = 1,000,000 requests per second
        let tau = 1u64;
        let limit = 1_000_000u64.checked_div(tau).unwrap_or(0).max(1);
        assert_eq!(limit, 1_000_000);
    }

    #[test]
    fn test_limit_calculation_zero_tau() {
        // For tau = 0 (edge case)
        let tau = 0u64;
        let limit = 1_000_000u64.checked_div(tau).unwrap_or(0).max(1);
        assert_eq!(limit, 1);
    }

    #[test]
    fn test_remaining_capacity_calculation() {
        let tau = 100_000u64; // 100ms
        let burst = 900_000u64; // 900ms (9 tokens worth)
        let t0 = 1_000_000u64; // current time
        let tat = 1_100_000u64; // TAT is 100ms ahead

        let total_capacity = burst + tau; // 1,000,000 (10 tokens)
        let used_capacity = tat.saturating_sub(t0); // 100,000 (1 token used)
        let remaining = total_capacity
            .saturating_sub(used_capacity)
            .checked_div(tau)
            .unwrap_or(0);

        assert_eq!(remaining, 9); // 9 tokens remaining
    }

    #[test]
    fn test_remaining_capacity_full_bucket() {
        let tau = 100_000u64;
        let burst = 900_000u64;
        let t0 = 1_000_000u64;
        let tat = t0; // TAT equals current time (full bucket)

        let total_capacity = burst + tau;
        let used_capacity = tat.saturating_sub(t0);
        let remaining = total_capacity
            .saturating_sub(used_capacity)
            .checked_div(tau)
            .unwrap_or(0);

        assert_eq!(remaining, 10); // Full 10 tokens
    }

    #[test]
    fn test_remaining_capacity_empty_bucket() {
        let tau = 100_000u64;
        let burst = 900_000u64;
        let t0 = 1_000_000u64;
        let tat = t0 + burst + tau; // TAT is fully ahead

        let total_capacity = burst + tau;
        let used_capacity = tat.saturating_sub(t0);
        let remaining = total_capacity
            .saturating_sub(used_capacity)
            .checked_div(tau)
            .unwrap_or(0);

        assert_eq!(remaining, 0); // No tokens remaining
    }

    #[test]
    fn test_allowed_at_calculation() {
        let burst = 900_000u64;
        let tat = 2_000_000u64;

        // allowed_at is when we can accept the next request
        let allowed_at = tat.saturating_sub(burst);
        assert_eq!(allowed_at, 1_100_000);
    }

    #[test]
    fn test_request_allowed_condition() {
        let t0 = 1_200_000u64; // Current time
        let tat = 2_000_000u64;
        let burst = 900_000u64;

        let allowed_at = tat.saturating_sub(burst); // 1,100,000

        // Request is allowed if t0 >= allowed_at
        let is_allowed = t0 >= allowed_at;
        assert!(is_allowed);
    }

    #[test]
    fn test_request_denied_condition() {
        let t0 = 1_000_000u64; // Current time (too early)
        let tat = 2_000_000u64;
        let burst = 900_000u64;

        let allowed_at = tat.saturating_sub(burst); // 1,100,000

        // Request is denied if t0 < allowed_at
        let is_denied = t0 < allowed_at;
        assert!(is_denied);
    }

    #[test]
    fn test_new_tat_calculation_on_accept() {
        let tau = 100_000u64;
        let t0 = 1_200_000u64;
        let tat = 1_100_000u64;

        // New TAT = max(tat, t0) + tau
        let new_tat = cmp::max(tat, t0) + tau;
        assert_eq!(new_tat, 1_300_000);
    }

    #[test]
    fn test_retry_after_calculation() {
        let t0 = 1_000_000u64;
        let tat = 2_000_000u64;
        let burst = 900_000u64;

        // retry_after = allowed_at - t0 = (tat - burst) - t0
        let wait_duration_micros = tat.saturating_sub(burst).saturating_sub(t0);
        assert_eq!(wait_duration_micros, 100_000); // 100ms wait
    }

    #[test]
    fn test_retry_after_zero_when_allowed() {
        let t0 = 1_200_000u64;
        let tat = 2_000_000u64;
        let burst = 900_000u64;

        let allowed_at = tat.saturating_sub(burst); // 1,100,000

        // When allowed, retry_after would be negative (clamped to 0 by saturating_sub)
        let wait_duration_micros = allowed_at.saturating_sub(t0);
        assert_eq!(wait_duration_micros, 0);
    }
}
