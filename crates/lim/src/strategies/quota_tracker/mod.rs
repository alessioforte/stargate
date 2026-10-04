mod time_window;

use crate::clock::CachedClock;
use crate::decision::RateLimitDecision;
use crate::error::Result;
use crate::state::State;
use crate::strategies::RateLimit;
use async_trait::async_trait;
use core::time::Duration;
use std::sync::Arc;

pub use time_window::TimeWindow;

// ========================== IN-MEMORY IMPLEMENTATION =========================

#[cfg(feature = "memory")]
pub struct QuotaTracker {
    /// in-memory store
    store: Arc<State>,

    /// clock used to measure time
    clock: Arc<CachedClock>,

    /// Maximum number of requests allowed in the time window
    limit: u64,

    /// Time window for the quota in seconds
    window: u64,
}

#[cfg(feature = "memory")]
impl QuotaTracker {
    pub fn new(store: Arc<State>, clock: Arc<CachedClock>, limit: u64, window: TimeWindow) -> Self {
        Self {
            store,
            clock,
            limit,
            window: window.duration().as_secs(),
        }
    }
}

#[cfg(feature = "memory")]
#[async_trait]
impl RateLimit for QuotaTracker {
    fn kind(&self) -> crate::strategies::LimitKind {
        crate::strategies::LimitKind::Quota
    }

    async fn check(&self, key: &str, cost: u64) -> Result<RateLimitDecision> {
        let now = self.clock.now_secs();
        let window = self.window;
        let start = (now / window) * window;
        let end = start + window;

        // Include window start in key to ensure proper window rotation
        let k = format!("{}:{}:{}", key, window, start);

        // TTL should be at least the window duration plus a small buffer
        // to ensure the key doesn't expire before the window ends
        let ttl = Some(window + 1);

        let reset = Some(Duration::from_secs(end - now));
        let retry_after = Some(Duration::from_secs(end - now));

        let limit = i64::try_from(self.limit).map_err(|_| {
            crate::error::RateLimitError::InvalidConfig(
                "Quota limit must not exceed i64::MAX".to_string(),
            )
        })?;

        // A cost that cannot fit in the store's integer type necessarily
        // exceeds the validated limit, so it can be denied without mutation.
        let Ok(cost) = i64::try_from(cost) else {
            return Ok(RateLimitDecision::denied(self.limit, 0, retry_after, reset));
        };

        let (allowed, count) = self
            .store
            .measure_and_set_i64(
                &k,
                |current| match current.checked_add(cost) {
                    Some(next) if current >= 0 && next <= limit => (true, next),
                    _ => (false, current),
                },
                ttl,
            )
            .await
            .map_err(|e| {
                crate::error::RateLimitError::MemoryError(format!(
                    "Failed to update quota counter: {}",
                    e,
                ))
            })?;

        let count = u64::try_from(count).map_err(|_| {
            crate::error::RateLimitError::MemoryError(
                "Quota counter contains a negative value".to_string(),
            )
        })?;

        if allowed {
            Ok(RateLimitDecision::allowed(
                self.limit,
                self.limit.saturating_sub(count),
                reset,
            ))
        } else {
            Ok(RateLimitDecision::denied(self.limit, 0, retry_after, reset))
        }
    }
}

// =========================== REDIS IMPLEMENTATION ===========================

#[cfg(feature = "redis")]
static QUOTA_TRACKER_LUA_SCRIPT: std::sync::LazyLock<store::RedisScript> =
    std::sync::LazyLock::new(|| store::RedisScript::new(include_str!("quota_tracker.lua")));

#[cfg(feature = "redis")]
pub struct QuotaTracker {
    /// Redis store
    store: Arc<State>,

    /// Maximum number of requests allowed in the time window
    limit: u64,

    /// Time window for the quota in seconds
    window: u64,
}

#[cfg(feature = "redis")]
impl QuotaTracker {
    pub fn new(
        store: Arc<State>,
        _clock: Arc<CachedClock>,
        limit: u64,
        window: TimeWindow,
    ) -> Self {
        Self {
            store,
            limit,
            window: window.duration().as_secs(),
        }
    }
}

#[cfg(feature = "redis")]
#[async_trait]
impl RateLimit for QuotaTracker {
    fn kind(&self) -> crate::strategies::LimitKind {
        crate::strategies::LimitKind::Quota
    }

    async fn check(&self, key: &str, cost: u64) -> Result<RateLimitDecision> {
        let mut con = self.store.get_connection();

        let script = &*QUOTA_TRACKER_LUA_SCRIPT;

        let limit = self.limit;
        let window = self.window;

        let (allowed, count, ttl_remaining): (i32, u64, u64) = script
            .key(key)
            .arg(limit)
            .arg(window)
            .arg(cost)
            .invoke_async(&mut con)
            .await
            .map_err(|e| {
                crate::error::RateLimitError::BackendError(format!("Redis script error: {}", e))
            })?;

        let reset = Some(Duration::from_secs(ttl_remaining));
        let remaining = limit.saturating_sub(count);

        if allowed == 1 {
            Ok(RateLimitDecision::allowed(limit, remaining, reset))
        } else {
            let retry_after = Some(Duration::from_secs(ttl_remaining));
            Ok(RateLimitDecision::denied(limit, 0, retry_after, reset))
        }
    }
}

#[cfg(test)]
mod tests {
    /// A denied check must not consume budget: after a cost-5 check is
    /// rejected with 3 units left, a cost-1 check on the same key still
    /// succeeds. Mirrors the Redis Lua script's check-then-increment.
    #[cfg(feature = "memory")]
    #[tokio::test]
    async fn denied_check_does_not_consume_remaining_budget() {
        use super::{QuotaTracker, TimeWindow};
        use crate::clock::CachedClock;
        use crate::state::State;
        use crate::strategies::RateLimit;
        use std::sync::Arc;

        let tracker = QuotaTracker::new(
            Arc::new(State::new()),
            CachedClock::new(),
            10,
            TimeWindow::Day,
        );

        let key = "quota:sub_123";

        // Consume 7 of 10.
        assert!(tracker.check(key, 7).await.unwrap().is_allowed());

        // Cost 5 exceeds the 3 remaining: denied...
        assert!(!tracker.check(key, 5).await.unwrap().is_allowed());

        // ...and must not have burned them: cost 1 still fits, three times.
        for expected_remaining in [2, 1, 0] {
            let decision = tracker.check(key, 1).await.unwrap();
            assert!(decision.is_allowed());
            assert_eq!(decision.remaining, expected_remaining);
        }
        assert!(!tracker.check(key, 1).await.unwrap().is_allowed());
    }

    #[test]
    fn test_time_window_in_key() {
        // Verify that different windows produce different aligned timestamps
        let timestamp = 1234567890u64;

        let minute_window = 60u64;
        let hour_window = 3600u64;

        let minute_start = (timestamp / minute_window) * minute_window;
        let hour_start = (timestamp / hour_window) * hour_window;

        assert_ne!(minute_start, hour_start);
        assert_eq!(minute_start % minute_window, 0);
        assert_eq!(hour_start % hour_window, 0);
    }
}
