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
    async fn check(&self, key: &str, cost: u64) -> Result<RateLimitDecision> {
        use store::AtomicStore;

        let now = self.clock.now_secs();
        let window = self.window;
        let start = (now / window) * window;
        let end = start + window;

        // Include window start in key to ensure proper window rotation
        let k = format!("{}:{}:{}", key, window, start);

        // TTL should be at least the window duration plus a small buffer
        // to ensure the key doesn't expire before the window ends
        let ttl = Some(window + 1);

        let count = match self.store.incr_i64(&k, cost as i64, ttl).await {
            Ok(Some(c)) => c as u64,
            _ => 0,
        };

        let reset = Some(Duration::from_secs(end - now));

        if count <= self.limit {
            Ok(RateLimitDecision::allowed(
                self.limit,
                self.limit.saturating_sub(count),
                reset,
            ))
        } else {
            let retry_after = Some(Duration::from_secs(end - now));
            Ok(RateLimitDecision::denied(self.limit, 0, retry_after, reset))
        }
    }
}

// =========================== REDIS IMPLEMENTATION ===========================

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
    async fn check(&self, key: &str, cost: u64) -> Result<RateLimitDecision> {
        let mut con = self.store.get_connection().await.map_err(|e| {
            crate::error::RateLimitError::BackendError(format!(
                "Failed to get Redis connection: {}",
                e
            ))
        })?;

        let script = store::RedisScript::new(include_str!("quota_tracker.lua"));

        let limit = self.limit;
        let window = self.window;

        let (allowed, count, ttl_remaining): (i32, u64, u64) = script
            .key(key)
            .arg(limit)
            .arg(window)
            .arg(cost)
            .invoke(&mut con)
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
