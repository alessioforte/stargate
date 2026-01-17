use core::num::NonZeroU32;
use core::time::Duration;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct Quota {
    pub max_burst: NonZeroU32,
    pub replenish_1_per: Duration,
}

impl Default for Quota {
    fn default() -> Self {
        Quota {
            max_burst: NonZeroU32::new(2).unwrap(),
            replenish_1_per: Duration::from_secs(5),
        }
    }
}

impl Quota {
    /// Create a quota allowing `count` requests per `duration`
    /// Example: `Quota::per_second(10)` → 10 req/s with burst of 10
    pub fn per_second(count: u32) -> Self {
        Self::per_duration(count, Duration::from_secs(1))
    }

    pub fn per_minute(count: u32) -> Self {
        Self::per_duration(count, Duration::from_secs(60))
    }

    pub fn per_hour(count: u32) -> Self {
        Self::per_duration(count, Duration::from_secs(3600))
    }

    pub fn per_duration(count: u32, duration: Duration) -> Self {
        let count = count.max(1);
        let replenish_1_per = duration / count;
        Self {
            max_burst: NonZeroU32::new(count).unwrap(),
            replenish_1_per,
        }
    }

    /// Set a custom burst size (different from rate)
    /// Example: Allow 100 req/s but burst up to 500
    pub fn with_burst(mut self, burst: u32) -> Self {
        self.max_burst = NonZeroU32::new(burst.max(1)).unwrap();
        self
    }

    /// Validate the quota configuration
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.replenish_1_per.is_zero() {
            return Err("replenish_1_per cannot be zero");
        }
        Ok(())
    }
}
