//! # Store - A unified storage interface with Redis-style hash operations
//!
//! This crate provides a unified interface for key-value and hash storage operations,
//! supporting both in-memory and Redis backends.
//!
//! ## Features
//!
//! - **Key-Value Operations**: Standard get, set, delete, exists operations
//! - **Hash Operations**: Redis-style hash operations (hset, hget, hdel, etc.)
//! - **TTL Support**: Time-to-live for both simple values and individual hash fields
//! - **Multiple Backends**: Memory store and Redis store implementations
//! - **Thread Safety**: All operations are async and thread-safe
//! - **Type Safety**: Generic operations with serde serialization
//!
//! ## Quick Start
//!
//! ```rust
//! use store::{MemoryStore, Store, StoreResult};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct User {
//!     name: String,
//!     email: String,
//! }
//!
//! #[tokio::main]
//! async fn main() -> StoreResult<()> {
//!     let store = MemoryStore::new();
//!
//!     // Key-value operations with error handling
//!     let user = User {
//!         name: "Alice".to_string(),
//!         email: "alice@example.com".to_string(),
//!     };
//!     store.set("user:1", &user, None).await?;
//!     let retrieved: Option<User> = store.get("user:1").await?;
//!
//!     // Hash operations with TTL and error handling
//!     store.hset("users", "alice", &user, Some(60)).await?; // 60 second TTL
//!     let user_from_hash: Option<User> = store.hget("users", "alice").await?;
//!
//!     // Check field existence
//!     let exists = store.hexists("users", "alice").await?;
//!
//!     // Get all users from hash
//!     let all_users: std::collections::HashMap<String, User> =
//!         store.hgetall("users").await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Hash Operations
//!
//! The store supports Redis-style hash operations:
//!
//! - `hset` - Set field in hash with optional TTL
//! - `hget` - Get field from hash
//! - `hdel` - Delete field from hash
//! - `hgetall` - Get all field-value pairs
//! - `hexists` - Check if field exists
//! - `hkeys` - Get all field names
//! - `hvals` - Get all values
//! - `hlen` - Get field count
//!
//! ## TTL (Time-To-Live) Support
//!
//! TTL support varies by backend:
//!
//! ```rust
//! # use store::{MemoryStore, Store, StoreResult};
//! # #[tokio::main]
//! # async fn main() -> StoreResult<()> {
//! # let store = MemoryStore::new();
//! // Set key with 30 second TTL (both MemoryStore and RedisStore)
//! store.set("temp_key", &"temporary data", Some(30)).await?;
//!
//! // Set hash field with 60 second TTL (both MemoryStore and RedisStore)
//! store.hset("cache", "temp_field", &"temporary data", Some(60)).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Backends
//!
//! ### MemoryStore (feature = "memory")
//! - In-memory storage using DashMap for thread safety
//! - Full TTL support for both keys and individual hash fields
//! - Automatic cleanup of expired entries with background cleaner
//! - No external dependencies
//!
//! ### RedisStore (feature = "redis")
//! - Redis backend using the redis crate
//! - Leverages Redis native hash operations for optimal performance
//! - Full TTL support for both keys and individual hash fields
//! - Network-based storage with Redis server
//! - Uses Redis's native `HSET_EX` command for per-field TTL
//!
//! ## Error Handling
//!
//! All store operations return `StoreResult<T>` which is a type alias for `Result<T, StoreError>`.
//! This provides comprehensive error handling for various failure scenarios:
//!
//! ```rust
//! use store::{MemoryStore, Store, StoreError};
//!
//! # #[tokio::main]
//! # async fn main() {
//! let store = MemoryStore::new();
//!
//! // Handle errors with match
//! match store.set("key", &"value", None).await {
//!     Ok(_) => println!("Success!"),
//!     Err(StoreError::InvalidInput(msg)) => println!("Invalid input: {}", msg),
//!     Err(StoreError::SerializationFailed(msg)) => println!("Serialization error: {}", msg),
//!     Err(e) => println!("Other error: {}", e),
//! }
//!
//! // Handle errors with ? operator
//! async fn example_function(store: &MemoryStore) -> Result<(), StoreError> {
//!     store.set("key", &"value", None).await?;
//!     let _value: Option<String> = store.get("key").await?;
//!     Ok(())
//! }
//! # }
//! ```
//!
//! ## Examples
//!
//! For complete examples, see the `examples/` directory:
//!
//! ```bash
//! # Basic error handling patterns
//! cargo run --example error_handling_demo --features memory
//!
//! # Compatibility wrappers for migration
//! cargo run --example compatibility_wrapper --features memory
//! ```

mod error;
mod store;

pub use error::{StoreError, StoreResult};
pub use store::Store;

#[cfg(feature = "memory")]
mod memory;
#[cfg(feature = "memory")]
pub use memory::MemoryStore;
#[cfg(feature = "memory")]
pub use memory::print_stats;

#[cfg(feature = "redis")]
mod redis;
#[cfg(feature = "redis")]
pub use redis::RedisStore;
