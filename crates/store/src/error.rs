//! Error types for store operations

use std::fmt;

/// Result type for store operations
pub type StoreResult<T> = Result<T, StoreError>;

/// Errors that can occur during store operations
#[derive(Debug, Clone)]
pub enum StoreError {
    /// Connection failed to the underlying storage backend
    ConnectionFailed(String),

    /// Serialization failed when converting data to storage format
    SerializationFailed(String),

    /// Deserialization failed when converting data from storage format
    DeserializationFailed(String),

    /// Redis-specific operation failed
    RedisFailed(String),

    /// Invalid input provided (e.g., empty key, invalid TTL)
    InvalidInput(String),

    /// Storage backend is unavailable or unreachable
    BackendUnavailable(String),

    /// Operation timed out
    Timeout(String),

    /// Generic I/O error occurred
    IoError(String),

    /// Type mismatch error
    TypeMismatch(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            StoreError::SerializationFailed(msg) => write!(f, "Serialization failed: {}", msg),
            StoreError::DeserializationFailed(msg) => write!(f, "Deserialization failed: {}", msg),
            StoreError::RedisFailed(msg) => write!(f, "Redis operation failed: {}", msg),
            StoreError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            StoreError::BackendUnavailable(msg) => write!(f, "Backend unavailable: {}", msg),
            StoreError::Timeout(msg) => write!(f, "Operation timed out: {}", msg),
            StoreError::IoError(msg) => write!(f, "I/O error: {}", msg),
            StoreError::TypeMismatch(msg) => write!(f, "Type mismatch error: {}", msg),
        }
    }
}

impl std::error::Error for StoreError {}

#[cfg(feature = "redis")]
impl From<redis::RedisError> for StoreError {
    fn from(error: redis::RedisError) -> Self {
        if error.is_timeout() {
            return StoreError::Timeout(format!("Redis operation timed out: {}", error));
        }
        match error.kind() {
            redis::ErrorKind::InvalidClientConfig => {
                StoreError::ConnectionFailed(format!("Invalid Redis configuration: {}", error))
            }
            redis::ErrorKind::AuthenticationFailed => {
                StoreError::ConnectionFailed(format!("Redis authentication failed: {}", error))
            }
            redis::ErrorKind::UnexpectedReturnType => {
                StoreError::TypeMismatch(format!("Unexpected return type from Redis: {}", error))
            }
            redis::ErrorKind::ClusterConnectionNotFound => StoreError::BackendUnavailable(format!(
                "Redis cluster connection not found: {}",
                error
            )),
            redis::ErrorKind::Server(kind) => match kind {
                redis::ServerErrorKind::BusyLoading => {
                    StoreError::BackendUnavailable(format!("Redis is loading data: {}", error))
                }
                redis::ServerErrorKind::ClusterDown => {
                    StoreError::BackendUnavailable(format!("Redis cluster is down: {}", error))
                }
                redis::ServerErrorKind::TryAgain => {
                    StoreError::Timeout(format!("Redis operation should be retried: {}", error))
                }
                _ => StoreError::RedisFailed(format!("Redis server error: {}", error)),
            },
            redis::ErrorKind::Parse => {
                StoreError::DeserializationFailed(format!("Redis parse error: {}", error))
            }
            redis::ErrorKind::Io => StoreError::IoError(format!("Redis I/O error: {}", error)),
            redis::ErrorKind::Extension => {
                StoreError::RedisFailed(format!("Redis extension error: {}", error))
            }
            redis::ErrorKind::Client => {
                StoreError::RedisFailed(format!("Redis client error: {}", error))
            }
            _ => StoreError::RedisFailed(format!("Redis error: {}", error)),
        }
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        if error.is_data() {
            StoreError::DeserializationFailed(format!("Invalid JSON data: {}", error))
        } else if error.is_syntax() {
            StoreError::DeserializationFailed(format!("JSON syntax error: {}", error))
        } else if error.is_eof() {
            StoreError::DeserializationFailed(format!("Unexpected end of JSON: {}", error))
        } else {
            StoreError::SerializationFailed(format!("JSON processing error: {}", error))
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        StoreError::IoError(format!("I/O operation failed: {}", error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let error = StoreError::ConnectionFailed("test connection".to_string());
        assert_eq!(error.to_string(), "Connection failed: test connection");

        let error = StoreError::Timeout("op".to_string());
        assert_eq!(error.to_string(), "Operation timed out: op");
    }

    #[cfg(feature = "redis")]
    #[test]
    fn test_redis_error_conversion() {
        let redis_error = redis::RedisError::from((redis::ErrorKind::Io, "connection lost"));
        let store_error = StoreError::from(redis_error);

        match store_error {
            StoreError::IoError(msg) => assert!(msg.contains("connection lost")),
            _ => panic!("Expected IoError variant"),
        }
    }

    #[test]
    fn test_serde_error_conversion() {
        let json_error = serde_json::from_str::<i32>("invalid json").unwrap_err();
        let store_error = StoreError::from(json_error);

        match store_error {
            StoreError::DeserializationFailed(_) => {}
            _ => panic!("Expected DeserializationFailed variant"),
        }
    }
}
