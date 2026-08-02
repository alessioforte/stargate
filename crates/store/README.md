# store

Async key-value store abstraction with pluggable backends. Provides a unified `Store` + `AtomicStore` trait interface implemented by two backends: an in-process `MemoryStore` (DashMap, edge/single-node) and a `RedisStore` (cluster mode).

## Features

| Feature       | Enables                          |
|---------------|----------------------------------|
| `memory`      | `MemoryStore` (DashMap backend)  |
| `redis`       | `RedisStore` (Redis backend)     |
| `compression` | LZ4 compression for `RedisStore` |

Select one backend per binary via Cargo features:

```toml
# Edge / single-node
store = { path = "crates/store", features = ["memory"] }

# Cluster
store = { path = "crates/store", features = ["redis"] }

# Redis with LZ4 compression
store = { path = "crates/store", features = ["redis", "compression"] }
```

## Architecture

```
crates/store/
├── src/
│   ├── lib.rs              # Public re-exports (feature-gated)
│   ├── store.rs            # Store + AtomicStore traits
│   ├── error.rs            # StoreError enum + StoreResult<T>
│   ├── memory/
│   │   ├── memory.rs       # MemoryStore impl (DashMap)
│   │   ├── config.rs       # MemoryStoreConfig + presets
│   │   ├── persistence.rs  # Save/load to binary (MsgPack) and JSON
│   │   └── stats.rs        # AtomicOperationStats, StorageStats, metrics
│   └── redis/
│       ├── redis.rs        # RedisStore impl (connection pool)
│       └── scripts.rs      # Lua scripts: CAS, INCR, DECR, CAS_I64
```

### Trait hierarchy

```
Store            — KV + hash operations (get/set/delete/exists, hset/hget/hdel/hgetall/…)
AtomicStore      — Integer operations (get_i64/set_i64/incr_i64/decr_i64/compare_and_swap_i64)
```

Both traits are `async` (via `async-trait`) and `Send + Sync`. Types used as values must implement `serde::Serialize` (via the blanket `SerializeValue`) and `serde::Deserialize` (via `DeserializeValue`).

Values are serialized to/from **MessagePack** (`rmp-serde`) in both backends.

### Value types (internal)

`MemoryStore` stores three distinct internal variants per key:

| Variant       | Maps to trait ops            | Notes                          |
|---------------|------------------------------|--------------------------------|
| `Simple`      | `get` / `set`                | Arc byte slice + optional TTL  |
| `Hash`        | `hset` / `hget` / …         | Nested `DashMap` per field     |
| `AtomicI64`   | `AtomicStore` ops            | `portable_atomic::AtomicI64`   |

Mixing types on the same key returns `StoreError::TypeMismatch`.

---

## Store trait

```rust
#[async_trait]
pub trait Store: Send + Sync {
    async fn ping(&self) -> StoreResult<()>;

    // Key-value
    async fn get<T: DeserializeValue>(&self, key: &str) -> StoreResult<Option<T>>;
    async fn set<T: SerializeValue>(&self, key: &str, value: &T, ttl: Option<u64>) -> StoreResult<()>;
    async fn set_if_absent<T: SerializeValue>(&self, key: &str, value: &T, ttl: Option<u64>) -> StoreResult<bool>;
    async fn delete(&self, key: &str) -> StoreResult<bool>;
    async fn exists(&self, key: &str) -> StoreResult<bool>;

    // Hash
    async fn hset<T: SerializeValue>(&self, key: &str, field: &str, value: &T, ttl: Option<u64>) -> StoreResult<bool>;
    async fn hget<T: DeserializeValue>(&self, key: &str, field: &str) -> StoreResult<Option<T>>;
    async fn hdel(&self, key: &str, field: &str) -> StoreResult<bool>;
    async fn hgetall<T: DeserializeValue>(&self, key: &str) -> StoreResult<HashMap<String, T>>;
    async fn hexists(&self, key: &str, field: &str) -> StoreResult<bool>;
    async fn hkeys(&self, key: &str) -> StoreResult<Vec<String>>;
    async fn hvals<T: DeserializeValue>(&self, key: &str) -> StoreResult<Vec<T>>;
    async fn hlen(&self, key: &str) -> StoreResult<usize>;

    // Optimistic concurrency — compares the *serialized* representation of
    // `expected`, so types must serialize deterministically (avoid HashMap fields)
    async fn compare_and_swap<T: SerializeValue>(&self, key: &str, expected: &T, new: &T, ttl: Option<u64>) -> StoreResult<bool>;
}
```

`ttl` is always in **seconds**. `None` = no expiry. `Some(0)` = error.

## AtomicStore trait

```rust
#[async_trait]
pub trait AtomicStore: Sized {
    async fn get_i64(&self, key: &str) -> StoreResult<Option<i64>>;
    async fn set_i64(&self, key: &str, value: i64, ttl: Option<u64>) -> StoreResult<()>;
    async fn incr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>>;
    async fn decr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>>;
    async fn compare_and_swap_i64(&self, key: &str, old: i64, new: i64, ttl: Option<u64>) -> StoreResult<bool>;
}
```

---

## MemoryStore

In-process store backed by `DashMap` (sharded `RwLock`s, no global lock). Designed for the **edge** deployment profile where a single binary handles all state.

### Construction

```rust
use store::MemoryStore;

// Defaults: initial_capacity=1000, time_caching=true, cleanup_batch=100
let store = MemoryStore::new();

// Preset configurations
let store = MemoryStore::with_config(MemoryStoreConfig::high_performance());
let store = MemoryStore::with_config(MemoryStoreConfig::memory_efficient());
let store = MemoryStore::with_config(MemoryStoreConfig::development());
```

### MemoryStoreConfig

| Field                | Default  | Purpose                                          |
|----------------------|----------|--------------------------------------------------|
| `initial_capacity`   | `1000`   | Pre-allocated DashMap buckets                    |
| `enable_time_caching`| `true`   | Cache `Utc::now()` for 50ms to reduce syscalls   |
| `cleanup_batch_size` | `100`    | Max entries scanned per manual cleanup call      |
| `enable_preallocation`| `true`  | Pre-allocate `HashMap` on `hgetall`              |
| `max_memory_usage`   | `0`      | Soft memory limit in bytes (0 = disabled)        |

### TTL and expiry

TTL is stored as an absolute `DateTime<Utc>` alongside each value (and per hash field). Expiry is **lazy**: entries are removed on first access after expiration. The adaptive cleaner handles background eviction.

### Adaptive background cleaner

```rust
// Starts a Tokio task. Interval adapts: halves when high expiry rate,
// doubles (up to 300s) when low expiry rate.
store.run_cleaner(60); // base interval in seconds
```

### Extra operations (not part of the `Store` trait)

Call these directly on `MemoryStore`. `RedisStore` provides the same four
batch operations (single MGET / pipelined round trips) with identical
semantics; `measure_and_set*` and the cleanup/observability helpers are
memory-only.

```rust
// Batch operations (amortise DashMap overhead / Redis round trips).
// Each fails fast on the first invalid key or unreadable value.
store.batch_get::<T>(&["key1", "key2"]).await?;   // Vec<Option<T>>
store.batch_set(&[("key1", &val, Some(300))]).await?; // ()
store.batch_delete(&["key1", "key2"]).await?;     // Vec<bool>
store.batch_exists(&["key1", "key2"]).await?;     // Vec<bool>

// Restore a snapshot into an already-initialized store (replaces contents, resets stats)
store.absorb(&restored_store);

// Atomic read-modify-write on a serializable value
// measure_fn receives current value (or default), returns (result_bool, new_value)
store.measure_and_set("key", default_val, |current| {
    let updated = /* transform */ current;
    (true, updated)
}, Some(ttl)).await?;

// Same for AtomicI64 — lock-free CAS loop internally
store.measure_and_set_i64("counter", |current| {
    (current < limit, current + 1)
}, Some(ttl)).await?;

// Manual cleanup
let removed = store.cleanup_expired().await;
let removed = store.batch_cleanup_expired(Some(500)).await;

// Observability
let stats   = store.get_storage_stats();
let metrics = store.get_performance_metrics();
let eff     = store.get_memory_efficiency();
let ratio   = store.get_cache_hit_ratio();
let tips    = store.get_optimization_recommendations();
let bytes   = store.get_memory_usage_bytes();
let keys    = store.get_total_keys();
let is_pressure = store.is_memory_pressure();
store.reset_stats();
```

### Persistence

`MemoryStore` can snapshot to disk and restore on startup. All writes use an atomic temp-file + rename pattern with fsync; snapshot files are created owner-only (0600) and created directories owner-only (0700) on Unix. Timestamped backups are pruned automatically — only the 5 most recent per file are kept. Values that cannot be represented in the snapshot format are skipped and logged at WARN level instead of being silently corrupted.

```rust
// Binary (MessagePack) — compact, preserves all metadata
store.save_to_binary("data/store.bin").await?;
let store = MemoryStore::load_from_binary("data/store.bin").await?;

// With automatic backup of the previous file
let backup = store.save_to_binary_with_backup("data/store.bin").await?;

// JSON — full fidelity (preserves TTLs and stats)
store.save_to_json("data/store.json").await?;
let store = MemoryStore::load_from_json("data/store.json").await?;

// Simple JSON — human-readable, no TTL metadata, AtomicI64 restored as Simple
store.export_to_simple_json("data/export.json").await?;

// Load with automatic fallback from simple JSON format
let store = MemoryStore::load_from_json_with_fallback("data/store.json").await?;

// Utility
let info = MemoryStore::file_info("data/store.bin").await?; // Option<(size, modified)>
let bk   = MemoryStore::backup_file("data/store.bin").await?; // Option<backup_path>
```

---

## RedisStore

Redis-backed implementation using a lock-free round-robin connection pool. Each slot is a `redis::aio::ConnectionManager` with its own TCP socket, enabling true parallel I/O.

Requires Redis >= 7.4 when `hset` is used with a TTL (per-field expiry via `HEXPIRE`); all other operations work on older versions.

### Construction

```rust
use store::{RedisStore, RedisPoolConfig};

// Default pool: 4 connections, no compression,
// 5s response timeout, 5s connection timeout
let store = RedisStore::new("redis://localhost:6379").await?;

// Custom pool size and timeouts
let config = RedisPoolConfig::new(8)
    .with_response_timeout(Some(std::time::Duration::from_secs(2)))
    .with_connection_timeout(Some(std::time::Duration::from_secs(2)));
let store = RedisStore::with_config("redis://localhost:6379", config).await?;
```

Timed-out commands fail with `StoreError::Timeout` instead of hanging callers when the server is unresponsive.

### Compression (optional, `compression` feature)

```rust
use store::{Compression, CompressionConfig};

let store = RedisStore::new("redis://localhost:6379")
    .await?
    .with_compression(CompressionConfig::new(Compression::Lz4).with_min_size(512));
```

Values below `min_size` bytes are stored uncompressed even with LZ4 enabled. A one-byte prefix (`0x00` / `0x01`) distinguishes compressed from uncompressed payloads — both readers and writers must use the same compression config for a given key namespace. Values with an unknown prefix fail loudly with `DeserializationFailed` (no silent legacy fallback): data written without compression must be migrated before enabling it.

### Lua scripts

Atomic operations that Redis does not provide natively are implemented as Lua scripts (loaded lazily via `LazyLock`):

| Script         | Purpose                                              |
|----------------|------------------------------------------------------|
| `CAS_SCRIPT`   | GET → compare → SET (with optional EX)               |
| `CAS_I64_SCRIPT`| GET → compare integer → SET (with optional EX)      |
| `INCR_SCRIPT`  | INCRBY + optional EXPIRE (used when TTL is supplied) |
| `DECR_SCRIPT`  | DECRBY + optional EXPIRE (used when TTL is supplied) |
| `HSET_WITH_TTL_SCRIPT` | HSET + HEXPIRE (used when `hset` TTL is supplied; Redis >= 7.4) |

`incr_i64` / `decr_i64` use native `INCRBY` / `DECRBY` when no TTL is needed (avoids Lua overhead). `hset` without TTL uses plain `HSET`. In both backends `hset` returns `true` only when the field was newly created.

### Raw connection access

```rust
let mut conn = store.get_connection(); // ConnectionManager (Arc clone)
let pool_n   = store.pool_size();
```

---

## Error types

```rust
pub enum StoreError {
    ConnectionFailed(String),
    SerializationFailed(String),
    DeserializationFailed(String),
    RedisFailed(String),        // Redis-specific failures
    InvalidInput(String),       // Empty key, zero TTL, …
    BackendUnavailable(String),
    Timeout(String),            // Response/connection timeout
    IoError(String),
    TypeMismatch(String),       // Wrong value type for the key
}
```

`From` impls: `redis::RedisError`, `serde_json::Error`, `std::io::Error`.

---

## Quick start

```rust
use store::{Store, AtomicStore, MemoryStore};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, PartialEq)]
struct Session { user_id: String, ip: String }

#[tokio::main]
async fn main() -> store::StoreResult<()> {
    let store = MemoryStore::new();
    store.run_cleaner(60);

    // Simple value with 1h TTL
    store.set("session:abc", &Session { user_id: "u1".into(), ip: "1.2.3.4".into() }, Some(3600)).await?;
    let s: Option<Session> = store.get("session:abc").await?;

    // Hash
    store.hset("user:u1", "email", &"alice@example.com", None).await?;
    let email: Option<String> = store.hget("user:u1", "email").await?;

    // Atomic counter
    store.set_i64("hits", 0, None).await?;
    store.incr_i64("hits", 1, None).await?;

    // Optimistic update
    let created = store.set_if_absent("lock", &"owner-1", Some(30)).await?;
    let swapped = store.compare_and_swap("cfg", &"old", &"new", None).await?;

    // Snapshot
    store.save_to_binary("state.bin").await?;

    Ok(())
}
```
