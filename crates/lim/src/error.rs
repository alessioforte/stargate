//! Error types for the rate limiting library

use thiserror::Error;

/// Error types that can occur during rate limiting operations
#[derive(Error, Debug)]
pub enum RateLimitError {
    /// Rate limit has been exceeded
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// Invalid configuration provided
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Backend storage error
    #[error("Backend error: {0}")]
    BackendError(String),

    /// Clock-related error
    #[error("Clock error: {0}")]
    ClockError(String),

    /// Redis-specific error (only available with redis-support feature)
    #[cfg(feature = "redis")]
    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),

    /// Memory-specific error (only available with memory-support feature)
    #[cfg(feature = "memory")]
    #[error("Memory error: {0}")]
    MemoryError(String),
}

/// Result type alias for rate limiting operations
pub type Result<T> = std::result::Result<T, RateLimitError>;
