pub mod gcra;
pub mod quota_tracker;
pub mod token_bucket;

use crate::decision::RateLimitDecision;
use crate::error::Result;

use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    Rate,
    Quota,
}

impl std::fmt::Display for LimitKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Rate => "rate_limit",
            Self::Quota => "quota",
        })
    }
}

#[async_trait]
pub trait RateLimit: Send + Sync {
    fn kind(&self) -> LimitKind;
    /// Whether the balance is durable (SQL) rather than store-backed. Durable
    /// quotas fail closed: a store failure must never admit traffic above the
    /// limit.
    fn is_durable(&self) -> bool {
        false
    }
    fn set_quota_config_version(&mut self, _version: i64) {}

    fn quota_fingerprint(&self) -> Option<&str> {
        None
    }

    async fn check(&self, key: &str, cost: u64) -> Result<RateLimitDecision>;
}

//
// Rate limiting algorithms and strategies
// 1. Leaky Bucket
// 2. GCRA (Generic Cell Rate Algorithm)
// 3. Token Bucket
// 4. Fixed Window Counter
// 5. Sliding Window Log
// 6. Sliding Window Counter
//
// NOTE:
//
// Spits seconds
// millis = 1 / 1000 = 10^-3
// micros = 1 / 1_000_000 = 10^-6
// nanos  = 1 / 1_000_000_000 = 10^-
//
