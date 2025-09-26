# Error Handling Guide

This guide explains the comprehensive error handling system in the store library and provides best practices for handling different types of errors.

## Overview

The store library uses a robust error handling system based on Rust's `Result` type. All store operations return `StoreResult<T>`, which is a type alias for `Result<T, StoreError>`.

## Error Types

### StoreError Variants

The `StoreError` enum covers all possible error scenarios:

```rust
pub enum StoreError {
    /// Connection failed to the underlying storage backend
    ConnectionFailed(String),

    /// Serialization failed when converting data to storage format
    SerializationFailed(String),

    /// Deserialization failed when converting data from storage format
    DeserializationFailed(String),

    /// Redis-specific operation failed
    RedisFailed(String),

    /// Key or field not found
    NotFound(String),

    /// Invalid input provided (e.g., empty key, invalid TTL)
    InvalidInput(String),

    /// Storage backend is unavailable or unreachable
    BackendUnavailable(String),

    /// Operation timed out
    Timeout(String),

    /// Generic I/O error occurred
    IoError(String),

    /// Unknown error occurred
    Unknown(String),
}
```

### Automatic Error Conversions

The library automatically converts common error types:

- `redis::RedisError` → Various `StoreError` variants based on error kind
- `serde_json::Error` → `SerializationFailed` or `DeserializationFailed`
- `std::io::Error` → `IoError`

## Error Handling Patterns

### 1. Using the `?` Operator (Recommended)

The most idiomatic way to handle errors in Rust:

```rust
use store::{MemoryStore, Store, StoreResult};

async fn example_function() -> StoreResult<()> {
    let store = MemoryStore::new();
    
    // Errors are automatically propagated up
    store.set("key", &"value", None).await?;
    let value: Option<String> = store.get("key").await?;
    
    Ok(())
}
```

### 2. Match Pattern for Detailed Error Handling

When you need to handle specific error types differently:

```rust
use store::{MemoryStore, Store, StoreError};

async fn detailed_error_handling() {
    let store = MemoryStore::new();
    
    match store.set("", &"value", None).await {
        Ok(_) => println!("Success!"),
        Err(StoreError::InvalidInput(msg)) => {
            eprintln!("Invalid input: {}", msg);
            // Handle validation errors specifically
        },
        Err(StoreError::SerializationFailed(msg)) => {
            eprintln!("Serialization error: {}", msg);
            // Handle serialization errors
        },
        Err(StoreError::ConnectionFailed(msg)) => {
            eprintln!("Connection error: {}", msg);
            // Maybe retry or use fallback
        },
        Err(e) => {
            eprintln!("Unexpected error: {}", e);
            // Generic error handling
        }
    }
}
```

### 3. Graceful Defaults with `unwrap_or_else`

When you want to provide fallback behavior:

```rust
async fn with_fallback() {
    let store = MemoryStore::new();
    
    let exists = store.exists("some_key").await.unwrap_or_else(|e| {
        eprintln!("Error checking key existence: {}", e);
        false // Default to false if we can't check
    });
    
    let value: Option<String> = store.get("key").await.unwrap_or_else(|e| {
        eprintln!("Error retrieving value: {}", e);
        None // Default to None if we can't retrieve
    });
}
```

### 4. Collecting Multiple Operations

When performing multiple operations and you want to collect all errors:

```rust
async fn batch_operations() -> Result<Vec<String>, Vec<StoreError>> {
    let store = MemoryStore::new();
    let keys = vec!["key1", "key2", "key3"];
    
    let mut results = Vec::new();
    let mut errors = Vec::new();
    
    for key in keys {
        match store.get::<String>(key).await {
            Ok(Some(value)) => results.push(value),
            Ok(None) => {}, // Key doesn't exist, skip
            Err(e) => errors.push(e),
        }
    }
    
    if errors.is_empty() {
        Ok(results)
    } else {
        Err(errors)
    }
}
```

## Input Validation Errors

The store validates inputs and returns `InvalidInput` errors for:

- Empty or whitespace-only keys
- Empty or whitespace-only hash fields
- Zero TTL values

```rust
// These will return InvalidInput errors:
store.set("", &"value", None).await; // Empty key
store.set("   ", &"value", None).await; // Whitespace-only key
store.hset("hash", "", &"value", None).await; // Empty field
store.set("key", &"value", Some(0)).await; // Zero TTL
```

## Redis-Specific Error Handling

When using RedisStore, additional error scenarios are handled:

```rust
#[cfg(feature = "redis")]
async fn redis_error_handling() {
    use store::RedisStore;
    
    // Connection errors
    match RedisStore::new("redis://invalid-host:6379") {
        Ok(store) => {
            // Use store...
        },
        Err(StoreError::ConnectionFailed(msg)) => {
            eprintln!("Failed to connect to Redis: {}", msg);
            // Maybe fallback to memory store or retry
        },
        Err(e) => eprintln!("Unexpected error: {}", e),
    }
}
```

## Error Recovery Strategies

### 1. Retry with Exponential Backoff

```rust
async fn retry_with_backoff<T>(
    mut operation: impl FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = StoreResult<T>>>>,
    max_attempts: u32,
) -> StoreResult<T> {
    let mut attempt = 0;
    
    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempt += 1;
                if attempt >= max_attempts {
                    return Err(e);
                }
                
                let delay = std::time::Duration::from_millis(100 * (1 << attempt));
                tokio::time::sleep(delay).await;
            }
        }
    }
}
```

### 2. Fallback to Alternative Storage

```rust
async fn with_fallback_storage(
    primary: &dyn Store,
    fallback: &dyn Store,
    key: &str,
) -> Option<String> {
    match primary.get(key).await {
        Ok(value) => value,
        Err(e) => {
            eprintln!("Primary storage failed: {}, trying fallback", e);
            fallback.get(key).await.unwrap_or_else(|e| {
                eprintln!("Fallback also failed: {}", e);
                None
            })
        }
    }
}
```

### 3. Circuit Breaker Pattern

```rust
struct CircuitBreaker {
    failure_count: std::sync::atomic::AtomicU32,
    last_failure: std::sync::Mutex<Option<std::time::Instant>>,
    threshold: u32,
    timeout: std::time::Duration,
}

impl CircuitBreaker {
    fn new(threshold: u32, timeout: std::time::Duration) -> Self {
        Self {
            failure_count: std::sync::atomic::AtomicU32::new(0),
            last_failure: std::sync::Mutex::new(None),
            threshold,
            timeout,
        }
    }
    
    async fn call<T>(
        &self,
        operation: impl std::future::Future<Output = StoreResult<T>>,
    ) -> StoreResult<T> {
        // Check if circuit is open
        if self.failure_count.load(std::sync::atomic::Ordering::Relaxed) >= self.threshold {
            if let Some(last) = *self.last_failure.lock().unwrap() {
                if last.elapsed() < self.timeout {
                    return Err(StoreError::BackendUnavailable(
                        "Circuit breaker is open".to_string()
                    ));
                }
            }
        }
        
        match operation.await {
            Ok(result) => {
                // Reset on success
                self.failure_count.store(0, std::sync::atomic::Ordering::Relaxed);
                Ok(result)
            }
            Err(e) => {
                // Increment failure count
                self.failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                *self.last_failure.lock().unwrap() = Some(std::time::Instant::now());
                Err(e)
            }
        }
    }
}
```

## Creating Custom Error Types

You can create application-specific error types that convert from `StoreError`:

```rust
#[derive(Debug)]
enum AppError {
    UserNotFound(String),
    ValidationError(String),
    StorageError(String),
    NetworkError(String),
}

impl From<StoreError> for AppError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::InvalidInput(msg) => AppError::ValidationError(msg),
            StoreError::NotFound(resource) => AppError::UserNotFound(resource),
            StoreError::ConnectionFailed(msg) | 
            StoreError::BackendUnavailable(msg) |
            StoreError::Timeout(msg) => AppError::NetworkError(msg),
            _ => AppError::StorageError(error.to_string()),
        }
    }
}

async fn app_function() -> Result<(), AppError> {
    let store = MemoryStore::new();
    
    // StoreError is automatically converted to AppError
    store.set("key", &"value", None).await?;
    
    Ok(())
}
```

## Logging and Monitoring

For production systems, implement comprehensive logging:

```rust
use log::{error, warn, info, debug};

async fn logged_operation(store: &dyn Store, key: &str) -> StoreResult<Option<String>> {
    debug!("Attempting to retrieve key: {}", key);
    
    match store.get(key).await {
        Ok(Some(value)) => {
            info!("Successfully retrieved key: {}", key);
            Ok(Some(value))
        },
        Ok(None) => {
            debug!("Key not found: {}", key);
            Ok(None)
        },
        Err(StoreError::ConnectionFailed(ref msg)) => {
            error!("Connection failed for key {}: {}", key, msg);
            // Maybe trigger alert or fallback
            Err(StoreError::ConnectionFailed(msg.clone()))
        },
        Err(e) => {
            warn!("Store operation failed for key {}: {}", key, e);
            Err(e)
        }
    }
}
```

## Testing Error Scenarios

Write tests for error conditions:

```rust
#[tokio::test]
async fn test_invalid_input_errors() {
    let store = MemoryStore::new();
    
    // Test empty key
    let result = store.set("", &"value", None).await;
    assert!(matches!(result, Err(StoreError::InvalidInput(_))));
    
    // Test zero TTL
    let result = store.set("key", &"value", Some(0)).await;
    assert!(matches!(result, Err(StoreError::InvalidInput(_))));
    
    // Test empty field
    let result = store.hset("hash", "", &"value", None).await;
    assert!(matches!(result, Err(StoreError::InvalidInput(_))));
}

#[tokio::test]
async fn test_serialization_errors() {
    // Test with types that can't be serialized
    // This would require creating a custom type that fails serialization
}
```

## Best Practices

1. **Always handle errors explicitly** - Don't use `unwrap()` in production code
2. **Use the `?` operator** for error propagation in most cases
3. **Match specific error types** when you need different handling logic
4. **Provide meaningful error messages** in your application errors
5. **Log errors appropriately** - use different log levels based on severity
6. **Implement retry logic** for transient errors
7. **Use circuit breakers** for external dependencies
8. **Test error scenarios** to ensure your error handling works correctly
9. **Monitor error rates** in production to detect issues early
10. **Document error conditions** in your API documentation

## Migration from Panic-Based Code

If you have existing code that used the old API (before error handling), you can use compatibility wrappers:

```rust
use store::StoreCompat; // From compatibility_wrapper example

async fn legacy_style_code() {
    let store = MemoryStore::new();
    
    // Old style - panics on error
    store.set_or_panic("key", &"value", None).await;
    let value: Option<String> = store.get_or_panic("key").await;
    
    // Gradually migrate to proper error handling
    match store.set("key2", &"value2", None).await {
        Ok(_) => println!("Success!"),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

This allows for gradual migration while maintaining backward compatibility.