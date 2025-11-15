pub mod gcra;

use crate::decision::RateLimitDecision;
use crate::error::Result;

use async_trait::async_trait;
#[async_trait]
pub trait RateLimit: Send + Sync {
    async fn check(&self, key: &str) -> Result<RateLimitDecision>;
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
