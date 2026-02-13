use super::scripts::{CAS_I64_SCRIPT, CAS_SCRIPT, DECR_SCRIPT, INCR_SCRIPT};
use crate::error::{StoreError, StoreResult};
use crate::store::{AtomicStore, DeserializeValue, SerializeValue, Store};
use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, HashFieldExpirationOptions, SetExpiry};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub use redis::Script as RedisScript;

// =============================================================================
// Compression
// =============================================================================

/// Compression algorithm used for serialized values before storing in Redis.
///
/// When enabled, values are compressed after serialization and decompressed
/// before deserialization. This reduces network traffic and Redis memory usage
/// at the cost of some CPU overhead.
///
/// **Important:** all readers and writers for a given key must agree on the
/// compression setting. Changing the compression algorithm without migrating
/// existing data will cause deserialization failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    /// No compression (default). Values are stored as raw serialized bytes.
    #[default]
    None,

    /// LZ4 compression via `lz4_flex`. Very fast with reasonable compression
    /// ratios – well suited for low-latency workloads.
    ///
    /// Requires the `compression` feature flag.
    #[cfg(feature = "compression")]
    Lz4,
}

/// Configuration that controls when compression is applied.
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// The compression algorithm to use.
    pub algorithm: Compression,

    /// Minimum payload size (in bytes, *after* serialization) below which
    /// compression is skipped. Small values rarely benefit from compression
    /// and the overhead can actually make them larger.
    ///
    /// Defaults to `256` bytes.
    pub min_size: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: Compression::None,
            min_size: 256,
        }
    }
}

impl CompressionConfig {
    /// Create a new compression config with the given algorithm and default
    /// minimum size (256 bytes).
    pub fn new(algorithm: Compression) -> Self {
        Self {
            algorithm,
            ..Default::default()
        }
    }

    /// Set the minimum payload size for compression.
    pub fn with_min_size(mut self, min_size: usize) -> Self {
        self.min_size = min_size;
        self
    }

    /// Compress a byte buffer according to this configuration.
    fn compress(&self, data: Vec<u8>) -> StoreResult<Vec<u8>> {
        match self.algorithm {
            Compression::None => Ok(data),
            #[cfg(feature = "compression")]
            Compression::Lz4 => {
                if data.len() < self.min_size {
                    return Ok(data);
                }
                Ok(lz4_flex::compress_prepend_size(&data))
            }
        }
    }

    /// Decompress a byte buffer according to this configuration.
    fn decompress(&self, data: &[u8]) -> StoreResult<Vec<u8>> {
        match self.algorithm {
            Compression::None => Ok(data.to_vec()),
            #[cfg(feature = "compression")]
            Compression::Lz4 => {
                if data.len() < self.min_size {
                    // Data was below the threshold at write time, so it was
                    // stored uncompressed. Return as-is.
                    return Ok(data.to_vec());
                }
                lz4_flex::decompress_size_prepended(data).map_err(|e| {
                    StoreError::DeserializationFailed(format!("LZ4 decompression failed: {}", e))
                })
            }
        }
    }
}

// =============================================================================
// Pool Configuration
// =============================================================================

/// Configuration for the Redis connection pool.
///
/// The pool maintains multiple `ConnectionManager` instances, each backed by its
/// own TCP socket to the Redis server. This enables truly parallel I/O across
/// connections rather than multiplexing everything through a single socket.
#[derive(Debug, Clone)]
pub struct RedisPoolConfig {
    /// Number of connections to maintain in the pool.
    ///
    /// Each connection is a separate `ConnectionManager` with its own TCP socket.
    /// A higher value allows more parallel I/O but consumes more file descriptors
    /// and Redis server connections.
    ///
    /// Defaults to `4`.
    pub pool_size: usize,
}

impl Default for RedisPoolConfig {
    fn default() -> Self {
        Self { pool_size: 4 }
    }
}

impl RedisPoolConfig {
    /// Create a new pool configuration with the given pool size.
    ///
    /// # Panics
    ///
    /// Will not panic here, but a pool size of 0 will be rejected at pool creation time.
    pub fn new(pool_size: usize) -> Self {
        Self { pool_size }
    }
}

// =============================================================================
// Connection Pool
// =============================================================================

/// A lightweight, lock-free connection pool that distributes work across N
/// `ConnectionManager` instances using round-robin selection.
///
/// Each `ConnectionManager` internally manages a single multiplexed TCP
/// connection with automatic reconnection. Cloning a `ConnectionManager` is
/// cheap (Arc-based) and shares the same underlying socket. By maintaining N
/// *separate* managers we get N distinct TCP sockets, enabling truly parallel
/// I/O to the Redis server.
///
/// The round-robin counter uses relaxed atomic ordering which is sufficient —
/// perfect distribution is not required, only reasonable spread.
#[derive(Clone)]
struct RedisPool {
    connections: Arc<Vec<ConnectionManager>>,
    next: Arc<AtomicUsize>,
}

impl RedisPool {
    /// Create a new pool with `pool_size` connections to the given Redis client.
    async fn new(client: &redis::Client, pool_size: usize) -> StoreResult<Self> {
        if pool_size == 0 {
            return Err(StoreError::InvalidInput(
                "Pool size must be at least 1".to_string(),
            ));
        }

        let mut connections = Vec::with_capacity(pool_size);
        for i in 0..pool_size {
            let conn = ConnectionManager::new(client.clone()).await.map_err(|e| {
                StoreError::ConnectionFailed(format!(
                    "Failed to create Redis connection {} of {}: {}",
                    i + 1,
                    pool_size,
                    e
                ))
            })?;
            connections.push(conn);
        }

        tracing::debug!(
            "Redis connection pool created with {} connection(s)",
            pool_size
        );

        Ok(Self {
            connections: Arc::new(connections),
            next: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Acquire a connection from the pool using round-robin selection.
    ///
    /// This is a non-blocking, lock-free operation. The returned
    /// `ConnectionManager` is a cheap clone (Arc bump) of one of the pooled
    /// managers, so the caller gets its own handle while the pool retains
    /// ownership.
    fn get(&self) -> ConnectionManager {
        let idx = self.next.fetch_add(1, Ordering::Relaxed) % self.connections.len();
        self.connections[idx].clone()
    }

    /// Return the number of connections in the pool.
    fn size(&self) -> usize {
        self.connections.len()
    }
}

// =============================================================================
// RedisStore
// =============================================================================

/// Redis-backed implementation of the [`Store`] and [`AtomicStore`] traits.
///
/// `RedisStore` maintains a pool of Redis connections for high-throughput,
/// parallel I/O. By default, the pool contains 4 connections; this can be
/// customised via [`RedisPoolConfig`] when using [`RedisStore::with_config`].
///
/// ## Connection Pool
///
/// Under the hood, each pool slot is a `redis::aio::ConnectionManager` — a
/// multiplexed, auto-reconnecting connection. Multiple slots mean multiple TCP
/// sockets, which allows the OS and Redis server to process requests in parallel
/// rather than serialising them through a single socket.
///
/// ## Examples
///
/// ```rust,no_run
/// use store::{RedisStore, RedisPoolConfig};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Default pool (4 connections)
/// let store = RedisStore::new("redis://localhost:6379").await?;
///
/// // Custom pool size
/// let config = RedisPoolConfig::new(8);
/// let store = RedisStore::with_config("redis://localhost:6379", config).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct RedisStore {
    pool: RedisPool,
    compression: CompressionConfig,
}

impl RedisStore {
    /// Create a new `RedisStore` with the default pool configuration (4 connections)
    /// and no compression.
    pub async fn new(url: &str) -> StoreResult<Self> {
        Self::with_config(url, RedisPoolConfig::default()).await
    }

    /// Create a new `RedisStore` with a custom pool configuration and no compression.
    pub async fn with_config(url: &str, config: RedisPoolConfig) -> StoreResult<Self> {
        if url.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "Redis URL cannot be empty".to_string(),
            ));
        }

        let client = redis::Client::open(url).map_err(|e| {
            StoreError::ConnectionFailed(format!("Failed to create Redis client: {}", e))
        })?;

        let pool = RedisPool::new(&client, config.pool_size).await?;

        tracing::info!(
            "Redis store created with pool of {} connection(s) for {}",
            pool.size(),
            url
        );

        Ok(Self {
            pool,
            compression: CompressionConfig::default(),
        })
    }

    /// Enable compression on this store.
    ///
    /// Returns `self` for builder-style chaining:
    ///
    /// ```rust,no_run
    /// # use store::{RedisStore, CompressionConfig, Compression};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = RedisStore::new("redis://localhost:6379")
    ///     .await?
    ///     .with_compression(CompressionConfig::new(Compression::None));
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_compression(mut self, config: CompressionConfig) -> Self {
        self.compression = config;
        self
    }

    /// Return a reference to the current compression configuration.
    pub fn compression(&self) -> &CompressionConfig {
        &self.compression
    }

    /// Get a connection from the pool for use with Lua scripts or raw commands.
    ///
    /// The returned `ConnectionManager` is a cheap Arc-clone of one of the
    /// pooled connections, selected via round-robin. It can be used directly
    /// with the `redis` crate's async command API.
    pub fn get_connection(&self) -> ConnectionManager {
        self.pool.get()
    }

    /// Return the number of connections in the pool.
    pub fn pool_size(&self) -> usize {
        self.pool.size()
    }

    /// Validate key is not empty
    fn validate_key(&self, key: &str) -> StoreResult<()> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        Ok(())
    }

    /// Validate field is not empty
    fn validate_field(&self, field: &str) -> StoreResult<()> {
        if field.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "Field cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    fn serialize<T: SerializeValue>(&self, value: &T) -> StoreResult<Vec<u8>> {
        let raw = rmp_serde::to_vec(value).map_err(|e| {
            StoreError::SerializationFailed(format!("Failed to serialize value: {}", e))
        })?;
        self.compression.compress(raw)
    }

    fn deserialize<T: DeserializeValue>(&self, data: &[u8]) -> StoreResult<T> {
        let decompressed = self.compression.decompress(data)?;
        rmp_serde::from_slice(&decompressed).map_err(|e| {
            StoreError::DeserializationFailed(format!("Failed to deserialize value: {}", e))
        })
    }
}

// =============================================================================
// Store trait implementation
// =============================================================================

#[async_trait]
impl Store for RedisStore {
    async fn get<T: DeserializeValue>(&self, key: &str) -> StoreResult<Option<T>> {
        self.validate_key(key)?;

        let mut con = self.pool.get();

        let value: Option<Vec<u8>> = con
            .get(key)
            .await
            .map_err(|e| StoreError::RedisFailed(format!("Failed to get key '{}': {}", key, e)))?;

        match value {
            Some(v) => {
                let deserialized = self.deserialize(&v)?;
                Ok(Some(deserialized))
            }
            None => Ok(None),
        }
    }

    async fn set<T: SerializeValue>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<()> {
        self.validate_key(key)?;

        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let mut con = self.pool.get();

        let serialized_value = self.serialize(value)?;

        if let Some(ttl) = ttl {
            let _: () = con.set_ex(key, serialized_value, ttl).await.map_err(|e| {
                StoreError::RedisFailed(format!(
                    "Failed to set key '{}' with TTL {}: {}",
                    key, ttl, e
                ))
            })?;
        } else {
            let _: () = con.set(key, serialized_value).await.map_err(|e| {
                StoreError::RedisFailed(format!("Failed to set key '{}': {}", key, e))
            })?;
        }

        Ok(())
    }

    async fn delete(&self, key: &str) -> StoreResult<bool> {
        self.validate_key(key)?;

        let mut con = self.pool.get();

        let result: i32 = con.del(key).await.map_err(|e| {
            StoreError::RedisFailed(format!("Failed to delete key '{}': {}", key, e))
        })?;

        Ok(result > 0)
    }

    async fn exists(&self, key: &str) -> StoreResult<bool> {
        self.validate_key(key)?;

        let mut con = self.pool.get();

        let result: bool = con.exists(key).await.map_err(|e| {
            StoreError::RedisFailed(format!("Failed to check existence of key '{}': {}", key, e))
        })?;

        Ok(result)
    }

    async fn hset<T: SerializeValue>(
        &self,
        key: &str,
        field: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool> {
        self.validate_key(key)?;
        self.validate_field(field)?;

        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let mut con = self.pool.get();

        let serialized_value = self.serialize(value)?;

        let hash_field_expiration_options = if let Some(ttl) = ttl {
            HashFieldExpirationOptions::default().set_expiration(SetExpiry::EX(ttl))
        } else {
            HashFieldExpirationOptions::default()
        };

        let fields_values = vec![(field, &serialized_value)];
        let result: i32 = con
            .hset_ex(key, &hash_field_expiration_options, &fields_values)
            .await
            .map_err(|e| {
                StoreError::RedisFailed(format!(
                    "Failed to set hash field '{}' in key '{}': {}",
                    field, key, e
                ))
            })?;

        Ok(result == 1)
    }

    async fn hget<T: DeserializeValue>(&self, key: &str, field: &str) -> StoreResult<Option<T>> {
        self.validate_key(key)?;
        self.validate_field(field)?;

        let mut con = self.pool.get();

        let value: Option<Vec<u8>> = con.hget(key, field).await.map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to get hash field '{}' from key '{}': {}",
                field, key, e
            ))
        })?;

        match value {
            Some(v) => {
                let deserialized = self.deserialize(&v)?;
                Ok(Some(deserialized))
            }
            None => Ok(None),
        }
    }

    async fn hdel(&self, key: &str, field: &str) -> StoreResult<bool> {
        self.validate_key(key)?;
        self.validate_field(field)?;

        let mut con = self.pool.get();

        let result: i32 = con.hdel(key, field).await.map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to delete hash field '{}' from key '{}': {}",
                field, key, e
            ))
        })?;

        Ok(result > 0)
    }

    async fn hgetall<T: DeserializeValue>(&self, key: &str) -> StoreResult<HashMap<String, T>> {
        self.validate_key(key)?;

        let mut con = self.pool.get();

        let hash_data: HashMap<String, Vec<u8>> = con.hgetall(key).await.map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to get all hash fields from key '{}': {}",
                key, e
            ))
        })?;

        let mut result = HashMap::with_capacity(hash_data.len());

        for (field, value) in &hash_data {
            let deserialized = self.deserialize(value).map_err(|e| {
                StoreError::DeserializationFailed(format!(
                    "Failed to deserialize field '{}' from key '{}': {}",
                    field, key, e
                ))
            })?;
            result.insert(field.clone(), deserialized);
        }

        Ok(result)
    }

    async fn hexists(&self, key: &str, field: &str) -> StoreResult<bool> {
        self.validate_key(key)?;
        self.validate_field(field)?;

        let mut con = self.pool.get();

        let result: bool = con.hexists(key, field).await.map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to check existence of hash field '{}' in key '{}': {}",
                field, key, e
            ))
        })?;

        Ok(result)
    }

    async fn hkeys(&self, key: &str) -> StoreResult<Vec<String>> {
        self.validate_key(key)?;

        let mut con = self.pool.get();

        let result: Vec<String> = con.hkeys(key).await.map_err(|e| {
            StoreError::RedisFailed(format!("Failed to get hash keys from key '{}': {}", key, e))
        })?;

        Ok(result)
    }

    async fn hvals<T: DeserializeValue>(&self, key: &str) -> StoreResult<Vec<T>> {
        self.validate_key(key)?;

        let mut con = self.pool.get();

        let values: Vec<Vec<u8>> = con.hvals(key).await.map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to get hash values from key '{}': {}",
                key, e
            ))
        })?;

        let mut result = Vec::with_capacity(values.len());

        for (index, value) in values.iter().enumerate() {
            let deserialized = self.deserialize(value.as_slice()).map_err(|e| {
                StoreError::DeserializationFailed(format!(
                    "Failed to deserialize value at index {} from key '{}': {}",
                    index, key, e
                ))
            })?;
            result.push(deserialized);
        }

        Ok(result)
    }

    async fn hlen(&self, key: &str) -> StoreResult<usize> {
        self.validate_key(key)?;

        let mut con = self.pool.get();

        let len: usize = con.hlen(key).await.map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to get hash length from key '{}': {}",
                key, e
            ))
        })?;

        Ok(len)
    }

    async fn compare_and_swap<T: SerializeValue + DeserializeValue + PartialEq>(
        &self,
        key: &str,
        expected: &T,
        new: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool> {
        self.validate_key(key)?;

        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let mut con = self.pool.get();

        let expected_serialized = self.serialize(expected)?;
        let new_serialized = self.serialize(new)?;

        let ttl_str = ttl.map_or("nil".to_string(), |t| t.to_string());

        let result: i32 = CAS_SCRIPT
            .key(key)
            .arg(expected_serialized)
            .arg(new_serialized)
            .arg(ttl_str)
            .invoke_async(&mut con)
            .await
            .map_err(|e| {
                StoreError::RedisFailed(format!(
                    "Failed to perform compare and swap on key '{}': {}",
                    key, e
                ))
            })?;

        Ok(result == 1)
    }
}

// =============================================================================
// AtomicStore trait implementation
// =============================================================================

#[async_trait]
impl AtomicStore for RedisStore {
    async fn get_i64(&self, key: &str) -> StoreResult<Option<i64>> {
        self.validate_key(key)?;

        let mut con = self.pool.get();

        let value: Option<i64> = con
            .get(key)
            .await
            .map_err(|e| StoreError::RedisFailed(format!("Failed to get key '{}': {}", key, e)))?;

        Ok(value)
    }

    async fn set_i64(&self, key: &str, value: i64, ttl: Option<u64>) -> StoreResult<()> {
        self.validate_key(key)?;

        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let mut con = self.pool.get();

        if let Some(ttl) = ttl {
            let _: () = con.set_ex(key, value, ttl).await.map_err(|e| {
                StoreError::RedisFailed(format!(
                    "Failed to set key '{}' with TTL {}: {}",
                    key, ttl, e
                ))
            })?;
        } else {
            let _: () = con.set(key, value).await.map_err(|e| {
                StoreError::RedisFailed(format!("Failed to set key '{}': {}", key, e))
            })?;
        }

        Ok(())
    }

    async fn incr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>> {
        self.validate_key(key)?;

        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let mut con = self.pool.get();

        // Fast path: use native INCRBY when no TTL is needed (avoids Lua overhead)
        if ttl.is_none() {
            let new_value: i64 = con.incr(key, by).await.map_err(|e| {
                StoreError::RedisFailed(format!(
                    "Failed to increment key '{}' by {}: {}",
                    key, by, e
                ))
            })?;
            return Ok(Some(new_value));
        }

        // Slow path: use Lua script for atomic INCRBY + EXPIRE
        let ttl_str = ttl.map_or("nil".to_string(), |t| t.to_string());

        let new_value: i64 = INCR_SCRIPT
            .key(key)
            .arg(by)
            .arg(ttl_str)
            .invoke_async(&mut con)
            .await
            .map_err(|e| {
                StoreError::RedisFailed(format!(
                    "Failed to increment key '{}' by {}: {}",
                    key, by, e
                ))
            })?;

        Ok(Some(new_value))
    }

    async fn decr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>> {
        self.validate_key(key)?;

        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let mut con = self.pool.get();

        // Fast path: use native DECRBY when no TTL is needed (avoids Lua overhead)
        if ttl.is_none() {
            let new_value: i64 = con.decr(key, by).await.map_err(|e| {
                StoreError::RedisFailed(format!(
                    "Failed to decrement key '{}' by {}: {}",
                    key, by, e
                ))
            })?;
            return Ok(Some(new_value));
        }

        // Slow path: use Lua script for atomic DECRBY + EXPIRE
        let ttl_str = ttl.map_or("nil".to_string(), |t| t.to_string());

        let new_value: i64 = DECR_SCRIPT
            .key(key)
            .arg(by)
            .arg(ttl_str)
            .invoke_async(&mut con)
            .await
            .map_err(|e| {
                StoreError::RedisFailed(format!(
                    "Failed to decrement key '{}' by {}: {}",
                    key, by, e
                ))
            })?;

        Ok(Some(new_value))
    }

    async fn compare_and_swap_i64(
        &self,
        key: &str,
        old: i64,
        new: i64,
        ttl: Option<u64>,
    ) -> StoreResult<bool> {
        self.validate_key(key)?;

        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let mut con = self.pool.get();

        let ttl_str = ttl.map_or("nil".to_string(), |t| t.to_string());

        let result: i32 = CAS_I64_SCRIPT
            .key(key)
            .arg(old)
            .arg(new)
            .arg(ttl_str)
            .invoke_async(&mut con)
            .await
            .map_err(|e| {
                StoreError::RedisFailed(format!(
                    "Failed to perform compare and swap on key '{}': {}",
                    key, e
                ))
            })?;

        Ok(result == 1)
    }
}
