use thiserror::Error;

/// Error types for rate limiting operations
#[derive(Error, Debug, Clone, PartialEq)]
pub enum RateLimitError {
    /// Rate limit exceeded for the given key
    #[error("Rate limit exceeded for key: {key}")]
    RateLimitExceeded { key: String },

    /// Invalid configuration provided
    #[error("Invalid rate limit configuration: {message}")]
    InvalidConfig { message: String },

    /// Storage backend error
    #[error("Storage error: {message}")]
    StorageError { message: String },

    /// Configuration not found for the given key
    #[error("Configuration not found for key: {key}")]
    ConfigNotFound { key: String },

    /// Invalid key format or empty key
    #[error("Invalid key: {key}")]
    InvalidKey { key: String },

    /// Internal error in rate limiting logic
    #[error("Internal rate limit error: {message}")]
    InternalError { message: String },

    /// Cleanup operation failed
    #[error("Cleanup failed: {message}")]
    CleanupError { message: String },

    /// Serialization/deserialization error
    #[error("Serialization error: {message}")]
    SerializationError { message: String },
}

impl RateLimitError {
    /// Create a rate limit exceeded error
    pub fn exceeded(key: &str) -> Self {
        Self::RateLimitExceeded {
            key: key.to_string(),
        }
    }

    /// Create an invalid config error
    pub fn invalid_config(message: &str) -> Self {
        Self::InvalidConfig {
            message: message.to_string(),
        }
    }

    /// Create a storage error
    pub fn storage(message: &str) -> Self {
        Self::StorageError {
            message: message.to_string(),
        }
    }

    /// Create a config not found error
    pub fn config_not_found(key: &str) -> Self {
        Self::ConfigNotFound {
            key: key.to_string(),
        }
    }

    /// Create an invalid key error
    pub fn invalid_key(key: &str) -> Self {
        Self::InvalidKey {
            key: key.to_string(),
        }
    }

    /// Create an internal error
    pub fn internal(message: &str) -> Self {
        Self::InternalError {
            message: message.to_string(),
        }
    }

    /// Create a cleanup error
    pub fn cleanup(message: &str) -> Self {
        Self::CleanupError {
            message: message.to_string(),
        }
    }

    /// Create a serialization error
    pub fn serialization(message: &str) -> Self {
        Self::SerializationError {
            message: message.to_string(),
        }
    }

    /// Check if this error is a rate limit exceeded error
    pub fn is_rate_limit_exceeded(&self) -> bool {
        matches!(self, Self::RateLimitExceeded { .. })
    }

    /// Check if this error is a configuration error
    pub fn is_config_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidConfig { .. } | Self::ConfigNotFound { .. }
        )
    }

    /// Check if this error is a storage error
    pub fn is_storage_error(&self) -> bool {
        matches!(self, Self::StorageError { .. })
    }

    /// Get the error category as a string
    pub fn category(&self) -> &'static str {
        match self {
            Self::RateLimitExceeded { .. } => "rate_limit_exceeded",
            Self::InvalidConfig { .. } => "invalid_config",
            Self::StorageError { .. } => "storage_error",
            Self::ConfigNotFound { .. } => "config_not_found",
            Self::InvalidKey { .. } => "invalid_key",
            Self::InternalError { .. } => "internal_error",
            Self::CleanupError { .. } => "cleanup_error",
            Self::SerializationError { .. } => "serialization_error",
        }
    }

    /// Get the associated key if applicable
    pub fn key(&self) -> Option<&str> {
        match self {
            Self::RateLimitExceeded { key } => Some(key),
            Self::ConfigNotFound { key } => Some(key),
            Self::InvalidKey { key } => Some(key),
            _ => None,
        }
    }
}

/// Result type for rate limiting operations
pub type Result<T> = std::result::Result<T, RateLimitError>;

/// Rate limit decision with additional metadata
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitDecision {
    /// Whether the request is allowed
    pub allowed: bool,

    /// Remaining tokens in the bucket
    pub remaining_tokens: f64,

    /// Maximum tokens (burst size)
    pub max_tokens: f64,

    /// Estimated time until next token is available (in milliseconds)
    pub retry_after_ms: Option<u64>,

    /// The rate limit configuration used
    pub config: crate::config::RateLimitConfig,
}

impl RateLimitDecision {
    /// Create an allowed decision
    pub fn allowed(
        remaining_tokens: f64,
        max_tokens: f64,
        config: crate::config::RateLimitConfig,
    ) -> Self {
        Self {
            allowed: true,
            remaining_tokens,
            max_tokens,
            retry_after_ms: None,
            config,
        }
    }

    /// Create a denied decision
    pub fn denied(
        remaining_tokens: f64,
        max_tokens: f64,
        retry_after_ms: u64,
        config: crate::config::RateLimitConfig,
    ) -> Self {
        Self {
            allowed: false,
            remaining_tokens,
            max_tokens,
            retry_after_ms: Some(retry_after_ms),
            config,
        }
    }

    /// Get retry after duration if available
    pub fn retry_after_duration(&self) -> Option<std::time::Duration> {
        self.retry_after_ms
            .map(|ms| std::time::Duration::from_millis(ms))
    }

    /// Get the utilization percentage (0.0 to 1.0)
    pub fn utilization(&self) -> f64 {
        if self.max_tokens == 0.0 {
            0.0
        } else {
            1.0 - (self.remaining_tokens / self.max_tokens)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RateLimitConfig;

    #[test]
    fn test_error_creation() {
        let error = RateLimitError::exceeded("test_key");
        assert!(error.is_rate_limit_exceeded());
        assert_eq!(error.key(), Some("test_key"));
        assert_eq!(error.category(), "rate_limit_exceeded");

        let error = RateLimitError::invalid_config("invalid value");
        assert!(error.is_config_error());
        assert_eq!(error.category(), "invalid_config");
    }

    #[test]
    fn test_rate_limit_decision() {
        let config = RateLimitConfig::default();

        let allowed = RateLimitDecision::allowed(15.0, 20.0, config.clone());
        assert!(allowed.allowed);
        assert_eq!(allowed.utilization(), 0.25);
        assert!(allowed.retry_after_duration().is_none());

        let denied = RateLimitDecision::denied(0.0, 20.0, 1000, config);
        assert!(!denied.allowed);
        assert_eq!(denied.utilization(), 1.0);
        assert_eq!(
            denied.retry_after_duration(),
            Some(std::time::Duration::from_millis(1000))
        );
    }
}
