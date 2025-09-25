# Storage Stats and Memory Usage

The `MemoryStore` provides comprehensive statistics and memory usage tracking capabilities to help you monitor and optimize your application's storage performance.

## Features

- **Operation Counters**: Track all storage operations (gets, sets, deletes, etc.)
- **Memory Usage Estimation**: Calculate approximate memory usage of stored data
- **Cache Performance**: Monitor cache hit/miss ratios
- **Key Statistics**: Count total keys, simple keys, hash keys, and hash fields
- **Automatic Cleanup Tracking**: Monitor expired entries cleaned by the background cleaner

## Getting Storage Statistics

### Basic Usage

```rust
use store::{MemoryStore, Store};

let store = MemoryStore::new();

// Perform some operations
store.set("user:1", &"Alice", None).await;
store.hset("cache", "key1", &"value1", Some(60)).await;

// Get comprehensive stats
let stats = store.get_storage_stats();
println!("Total keys: {}", stats.total_keys);
println!("Memory usage: {} bytes", stats.estimated_memory_bytes);
println!("Cache hits: {}", stats.operations.cache_hits);
```

### Storage Stats Structure

```rust
pub struct StorageStats {
    pub total_keys: usize,             // Total number of keys (simple + hash)
    pub simple_keys: usize,            // Number of simple key-value pairs
    pub hash_keys: usize,              // Number of hash keys
    pub total_hash_fields: usize,      // Total fields across all hashes
    pub estimated_memory_bytes: usize, // Approximate memory usage
    pub operations: OperationStats,    // Detailed operation counters
}
```

### Operation Statistics

```rust
pub struct OperationStats {
    pub gets: u64,                     // Simple get operations
    pub sets: u64,                     // Simple set operations
    pub deletes: u64,                  // Simple delete operations
    pub exists_checks: u64,            // Existence checks
    pub hash_gets: u64,                // Hash field get operations
    pub hash_sets: u64,                // Hash field set operations
    pub hash_deletes: u64,             // Hash field delete operations
    pub hash_exists_checks: u64,       // Hash field existence checks
    pub hash_getalls: u64,             // Hash getall operations
    pub hash_keys_calls: u64,          // Hash keys operations
    pub hash_vals_calls: u64,          // Hash values operations
    pub hash_len_calls: u64,           // Hash length operations
    pub cache_hits: u64,               // Successful cache lookups
    pub cache_misses: u64,             // Failed cache lookups
    pub expired_entries_cleaned: u64,  // Entries cleaned by background cleaner
}
```

## Memory Usage Tracking

### Get Memory Usage

```rust
// Get total memory usage in bytes
let memory_bytes = store.get_memory_usage_bytes();
let memory_mb = memory_bytes as f64 / 1024.0 / 1024.0;
println!("Memory usage: {:.2} MB", memory_mb);

// Get total key count
let total_keys = store.get_total_keys();
println!("Total keys: {}", total_keys);
```

### Memory Estimation Details

The memory estimation includes:
- Key names (String storage)
- Simple values (serialized JSON strings)
- Hash map structures
- Hash field names and values
- TTL timestamps
- Internal data structure overhead

**Note**: Memory estimation is approximate and includes overhead from Rust data structures. Actual memory usage may vary.

## Cache Performance Monitoring

### Cache Hit Ratio

```rust
// Get cache hit ratio (0.0 to 1.0)
let hit_ratio = store.get_cache_hit_ratio();
println!("Cache hit ratio: {:.2}%", hit_ratio * 100.0);

// Operations that count as cache hits:
// - Successful get operations (non-expired data found)
// - Successful hget operations (non-expired field found)

// Operations that count as cache misses:
// - get operations returning None (key not found or expired)
// - hget operations returning None (field not found or expired)
// - Attempting to get hash data as simple value (type mismatch)
```

## Resetting Statistics

```rust
// Reset all operation counters to zero
// (Does not affect actual stored data or memory usage)
store.reset_stats();

// Check that counters are reset
let stats = store.get_storage_stats();
assert_eq!(stats.operations.gets, 0);
assert_eq!(stats.operations.cache_hits, 0);
```

## Background Cleaner Integration

The storage stats automatically track expired entries cleaned by the background cleaner:

```rust
// Start background cleaner with 30-second intervals
store.run_cleaner(30);

// After some time, check how many expired entries were cleaned
let stats = store.get_storage_stats();
println!("Expired entries cleaned: {}", stats.operations.expired_entries_cleaned);
```

## Practical Examples

### Monitoring Application Performance

```rust
use tokio::time::{interval, Duration};

// Monitor stats every minute
let mut ticker = interval(Duration::from_secs(60));
loop {
    ticker.tick().await;

    let stats = store.get_storage_stats();
    let memory_mb = stats.estimated_memory_bytes as f64 / 1024.0 / 1024.0;
    let hit_ratio = store.get_cache_hit_ratio();

    log::info!(
        "Store Stats - Keys: {}, Memory: {:.2}MB, Hit Ratio: {:.1}%, Operations: {}",
        stats.total_keys,
        memory_mb,
        hit_ratio * 100.0,
        stats.operations.gets + stats.operations.sets
    );
}
```

### Memory Usage Alerts

```rust
const MAX_MEMORY_MB: f64 = 100.0;

let memory_bytes = store.get_memory_usage_bytes();
let memory_mb = memory_bytes as f64 / 1024.0 / 1024.0;

if memory_mb > MAX_MEMORY_MB {
    log::warn!("High memory usage: {:.2}MB (limit: {}MB)", memory_mb, MAX_MEMORY_MB);

    // Consider cleaning up expired entries or implementing eviction
    let stats = store.get_storage_stats();
    log::info!("Current stats: {} keys, {} hash fields",
               stats.total_keys, stats.total_hash_fields);
}
```

### Performance Optimization

```rust
// Check cache performance periodically
let hit_ratio = store.get_cache_hit_ratio();

if hit_ratio < 0.8 {  // Less than 80% hit ratio
    log::warn!("Low cache hit ratio: {:.1}%", hit_ratio * 100.0);

    let stats = store.get_storage_stats();
    log::info!("Cache misses: {}, Cache hits: {}",
               stats.operations.cache_misses,
               stats.operations.cache_hits);

    // Consider:
    // - Adjusting TTL values
    // - Preloading frequently accessed data
    // - Analyzing access patterns
}
```

## Thread Safety

All statistics operations are thread-safe and can be called concurrently from multiple threads. The internal counters use atomic operations for performance and consistency.

## Serialization

The `StorageStats` and `OperationStats` structures implement `Serialize` and `Deserialize`, making them suitable for:
- JSON API responses
- Metrics collection systems
- Configuration files
- Logging and monitoring tools

```rust
use serde_json;

let stats = store.get_storage_stats();
let json = serde_json::to_string_pretty(&stats)?;
println!("Stats JSON:\n{}", json);
```
