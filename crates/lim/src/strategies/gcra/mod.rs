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

#[cfg(feature = "memory")]
pub struct Gcra {
    store: Arc<State>,
    clock: quanta::Clock,
    start: quanta::Instant,
    tau: u64,
    burst: u64,
}

#[cfg(feature = "memory")]
impl Gcra {
    pub fn new(store: Arc<State>, quota: Quota) -> Self {
        let tau = cmp::max(quota.replenish_1_per, Duration::from_micros(1)).as_micros() as u64;
        let burst = tau * (quota.max_burst.get() - 1) as u64;
        let clock = quanta::Clock::new();
        let start = clock.now();
        Gcra {
            store,
            clock,
            start,
            tau,
            burst,
        }
    }
}

#[cfg(feature = "redis")]
pub struct Gcra {
    store: Arc<State>,
    tau: u64,
    burst: u64,
}

#[cfg(feature = "redis")]
impl Gcra {
    pub fn new(store: Arc<State>, quota: Quota) -> Self {
        let tau = cmp::max(quota.replenish_1_per, Duration::from_micros(1)).as_micros() as u64;
        let burst = tau * (quota.max_burst.get() - 1) as u64;
        Gcra { store, tau, burst }
    }
}

#[async_trait]
impl RateLimit for Gcra {
    #[cfg(feature = "memory")]
    async fn check(&self, key: &str) -> Result<RateLimitDecision> {
        let now = self.clock.now();
        let t0 = now.duration_since(self.start).as_micros() as u64;

        let tau = self.tau;
        let burst = self.burst;

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
                Some(60),
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
        let limit = Duration::from_micros(tau).as_secs().max(1); // at least 1 second
        let reset = if tat > t0 { (tat - t0) / limit } else { 0 };
        let remaining = (tat - t0 - tau).max(0);

        if allow {
            Ok(RateLimitDecision::allowed(limit, remaining, reset))
        } else {
            let retry_after = tat.saturating_sub(burst) - t0;
            let retry_after = Duration::from_micros(retry_after);
            Ok(RateLimitDecision::denied(
                limit,
                remaining,
                reset,
                Some(retry_after),
            ))
        }
    }

    #[cfg(feature = "redis")]
    async fn check(&self, key: &str) -> Result<RateLimitDecision> {
        let mut con = self.store.get_connection().await.unwrap();
        let script = store::RedisScript::new(include_str!("gcra.lua"));

        let tau = self.tau;
        let burst = self.burst;
        let (allow, retry_after, remaining_burst): (i32, u64, u64) = script
            .key(key)
            .arg(tau)
            .arg(burst)
            .arg(60)
            .invoke(&mut con)
            .map_err(|e| {
                log::error!("Redis script error: {}", e);
            })
            .unwrap();

        let limit = Duration::from_micros(tau).as_secs().max(1);
        let remaining = if remaining_burst > 0 {
            (remaining_burst as u64) / limit
        } else {
            0
        };
        let reset = if retry_after > 0 {
            (retry_after as u64) / limit
        } else {
            0
        };
        if allow == 1 {
            Ok(RateLimitDecision::allowed(limit, remaining, reset))
        } else {
            let wait_duration = Duration::from_micros(retry_after);
            let retry_after = wait_duration;
            Ok(RateLimitDecision::denied(
                limit,
                remaining,
                reset,
                Some(retry_after),
            ))
        }
    }
}
