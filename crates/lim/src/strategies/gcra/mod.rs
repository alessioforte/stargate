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
    clock: quanta::Clock,

    /// start time of the token bucket
    start: quanta::Instant,

    /// tau is the time it takes to replenish one token in microseconds
    tau: u64,

    /// delay variation tolerance in microseconds
    burst: u64,

    /// ttl is the time to live for the token bucket in seconds
    ttl: u64,
}

#[cfg(feature = "memory")]
impl Gcra {
    pub fn new(store: Arc<State>, quota: Quota) -> Self {
        let tau = cmp::max(quota.replenish_1_per, Duration::from_micros(1)).as_micros() as u64;
        let burst = tau * (quota.max_burst.get() - 1) as u64;
        let clock = quanta::Clock::new();
        let start = clock.now();
        let ttl = ((tau * quota.max_burst.get() as u64) / 1_000_000).max(60);
        Gcra {
            store,
            clock,
            start,
            tau,
            burst,
            ttl,
        }
    }
}

#[cfg(feature = "memory")]
#[async_trait]
impl RateLimit for Gcra {
    async fn check(&self, key: &str) -> Result<RateLimitDecision> {
        let now = self.clock.now();
        let t0 = now.duration_since(self.start).as_micros() as u64;

        let tau = self.tau;
        let burst = self.burst;
        let ttl = self.ttl;
        println!("now: {:?}", now);
        println!("Tau: {}, Burst: {}, TTL: {}", tau, burst, ttl);

        let (allow, tat) = match self
            .store
            .measure_and_set_i64(
                key,
                |tat| {
                    let tat = tat as u64;
                    let allowed_at = tat.saturating_sub(burst);
                    if t0 < allowed_at {
                        // reject
                        (false, tat as i64)
                    } else {
                        // accept
                        let next = cmp::max(tat, t0) + tau;
                        (true, next as i64)
                    }
                },
                Some(ttl),
            )
            .await
        {
            Ok(result) => result,
            Err(e) => {
                return Err(crate::error::RateLimitError::MemoryError(format!(
                    "Failed to access in-memory store: {}",
                    e,
                )));
            }
        };

        let tat = tat as u64;

        // Calculate rate limit (requests per second)
        // tau is microseconds per request, so requests per second = 1_000_000 / tau
        let limit = if tau > 0 { (1_000_000 / tau).max(1) } else { 1 };

        // Calculate remaining capacity in burst
        // remaining_burst is how much burst capacity (in microseconds) is left
        let remaining_burst = (tat.saturating_sub(t0)).saturating_sub(tau);

        // Convert remaining burst capacity to number of requests
        let remaining = if tau > 0 && remaining_burst > 0 {
            remaining_burst / tau
        } else {
            0
        };

        println!("Remaining burst:    {}", remaining_burst);
        println!("Remaining requests: {}", remaining);

        if allow {
            Ok(RateLimitDecision::allowed(limit, remaining))
        } else {
            let wait_duration_micros = tat.saturating_sub(burst) - t0;
            let retry_after = Duration::from_micros(wait_duration_micros);
            Ok(RateLimitDecision::denied(
                limit,
                remaining,
                Some(retry_after),
            ))
        }
    }
}

// =========================== REDIS IMPLEMENTATION ===========================

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
    pub fn new(store: Arc<State>, quota: Quota) -> Self {
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
    async fn check(&self, key: &str) -> Result<RateLimitDecision> {
        let mut con = self.store.get_connection().await.map_err(|e| {
            crate::error::RateLimitError::BackendError(format!(
                "Failed to get Redis connection: {}",
                e
            ))
        })?;

        let script = store::RedisScript::new(include_str!("gcra.lua"));

        let tau = self.tau;
        let burst = self.burst;
        let ttl = self.ttl;

        let (allow, retry_after, remaining_burst): (i32, u64, u64) = script
            .key(key)
            .arg(tau)
            .arg(burst)
            .arg(ttl)
            .invoke(&mut con)
            .map_err(|e| {
                tracing::error!("Redis script error: {}", e);
            })
            .unwrap();

        // Calculate rate limit (requests per second)
        // tau is microseconds per request, so requests per second = 1_000_000 / tau
        let limit = if tau > 0 { (1_000_000 / tau).max(1) } else { 1 };

        // Convert remaining burst capacity (microseconds) to number of requests
        let remaining = if tau > 0 && remaining_burst > 0 {
            remaining_burst / tau
        } else {
            0
        };

        if allow == 1 {
            Ok(RateLimitDecision::allowed(limit, remaining))
        } else {
            let wait_duration = Duration::from_micros(retry_after);
            let retry_after = wait_duration;
            Ok(RateLimitDecision::denied(
                limit,
                remaining,
                Some(retry_after),
            ))
        }
    }
}
