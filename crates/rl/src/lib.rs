pub mod config;
pub mod error;
pub mod ip;
pub mod limiter;
pub mod quota;
pub mod quota_manager;
mod storage;
pub mod token_bucket;
pub mod unified_limiter;

pub use config::{GlobalRateLimitSettings, RateLimitConfig};
pub use error::{RateLimitDecision, RateLimitError, Result};
pub use limiter::{RateLimiter, RateLimiterBuilder};
pub use quota::{
    QuotaConfig, QuotaDecision, QuotaPeriodStatus, QuotaStatus, QuotaTracker, QuotaViolationType,
};
pub use quota_manager::{QuotaManager, QuotaManagerBuilder};
pub use token_bucket::TokenBucket;
pub use unified_limiter::{
    DenialReason, LimiterStatus, ResetResult, UnifiedDecision, UnifiedLimiter,
    UnifiedLimiterBuilder,
};

// Re-export commonly used types
pub use std::net::IpAddr;
pub use std::time::Duration;
