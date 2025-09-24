# Hash Operations in Store

This document describes the Redis-style hash operations that have been added to the Store trait and its implementations.

## Overview

Hash operations allow you to store and manipulate hash maps (key-value pairs) within a single key in the store. This is similar to Redis hash data types, where you can have a key that contains multiple field-value pairs.

## Available Hash Operations

### `hset<T>(key: &str, field: &str, value: &T, ttl: Option<u64>) -> bool`
Sets a field in a hash stored at the given key with optional TTL (time-to-live) in seconds.
- Returns `true` if the field is new
- Returns `false` if the field was updated
- Creates the hash if it doesn't exist
- `ttl`: Optional expiration time in seconds for the field (MemoryStore only)

```rust
// Set field without expiration
let is_new = store.hset("users", "alice", &user_data, None).await;

// Set field with 60 second expiration
let is_new = store.hset("users", "temp_user", &temp_data, Some(60)).await;
```

### `hget<T>(key: &str, field: &str) -> Option<T>`
Gets the value of a field from a hash.
- Returns `Some(value)` if the field exists
- Returns `None` if the field or hash doesn't exist

```rust
let user: Option<User> = store.hget("users", "alice").await;
```

### `hdel(key: &str, field: &str) -> bool`
Deletes a field from a hash.
- Returns `true` if the field was deleted
- Returns `false` if the field didn't exist

```rust
let deleted = store.hdel("users", "alice").await;
```

### `hgetall<T>(key: &str) -> HashMap<String, T>`
Gets all field-value pairs from a hash.
- Returns a HashMap with all fields and their values
- Returns an empty HashMap if the hash doesn't exist

```rust
let all_users: HashMap<String, User> = store.hgetall("users").await;
```

### `hexists(key: &str, field: &str) -> bool`
Checks if a field exists in a hash.
- Returns `true` if the field exists
- Returns `false` if the field or hash doesn't exist

```rust
let exists = store.hexists("users", "alice").await;
```

### `hkeys(key: &str) -> Vec<String>`
Gets all field names from a hash.
- Returns a vector of all field names
- Returns an empty vector if the hash doesn't exist

```rust
let field_names = store.hkeys("users").await;
```

### `hvals<T>(key: &str) -> Vec<T>`
Gets all values from a hash.
- Returns a vector of all values
- Returns an empty vector if the hash doesn't exist

```rust
let user_values: Vec<User> = store.hvals("users").await;
```

### `hlen(key: &str) -> usize`
Gets the number of fields in a hash.
- Returns the count of fields
- Returns 0 if the hash doesn't exist

```rust
let field_count = store.hlen("users").await;
```

## TTL (Time-To-Live) Support

### Per-Field TTL
Individual hash fields can have their own expiration times:
- **MemoryStore**: Full support for per-field TTL with automatic cleanup
- **RedisStore**: Full support for per-field TTL using Redis's native `hset_ex` command

### TTL Behavior
- **MemoryStore**: Expired fields are automatically excluded from all hash operations in real-time
  - `hget` returns `None` for expired fields
  - `hexists` returns `false` for expired fields
  - `hgetall`, `hkeys`, `hvals`, and `hlen` skip expired fields
  - Background cleanup process removes expired fields automatically
- **RedisStore**: TTL is handled natively by Redis server
  - Uses Redis's `hset_ex` command for setting field expiration
  - Redis handles expiration automatically on the server side
  - All hash operations respect Redis's native TTL behavior

### Example
```rust
// Set a field that expires in 30 seconds (both stores)
let memory_store = MemoryStore::new();
memory_store.hset("cache", "temp_data", &data, Some(30)).await;

let redis_store = RedisStore::new("redis://localhost").unwrap();
redis_store.hset("cache", "temp_data", &data, Some(30)).await;

// Field exists immediately in both stores
assert!(memory_store.hexists("cache", "temp_data").await);
assert!(redis_store.hexists("cache", "temp_data").await);

// After 30 seconds, field is automatically expired in both stores
// Both will return false after expiration
```

## Data Type Separation

Hash operations and regular key-value operations are kept separate:

- **Hash keys** can only be operated on with hash operations (`hset`, `hget`, etc.)
- **Simple keys** can only be operated on with regular operations (`set`, `get`, etc.)

Attempting to use hash operations on a simple key or vice versa will return empty results or `None`.

## Implementation Details

### MemoryStore
- Uses an internal enum `StoreValue` to distinguish between simple values and hash maps
- Hash data is stored using `DashMap<String, (String, Option<DateTime<Utc>>)>` for thread-safe concurrent access with per-field TTL
- Supports TTL (time-to-live) for both simple values and individual hash fields
- Expired entries are handled automatically by background cleanup and real-time checks

### RedisStore
- Uses native Redis hash commands (`HSET_EX`, `HGET`, etc.)
- All values are JSON-serialized before storage
- Leverages Redis's native hash data type for optimal performance
- **TTL Support**: Full per-field TTL support using Redis's `HSET_EX` command
- Redis handles expiration automatically on the server side

## Example Usage

```rust
use store::{MemoryStore, Store};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
struct User {
    id: u32,
    name: String,
    email: String,
}

#[tokio::main]
async fn main() {
    let store = MemoryStore::new();
    
    let user = User {
        id: 1,
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
    };
    
    // Set a user in the hash (no expiration)
    store.hset("users", "alice", &user, None).await;
    
    // Set a temporary user that expires in 60 seconds (works with both MemoryStore and RedisStore)
    store.hset("users", "temp", &user, Some(60)).await;
    
    // Get the user back
    if let Some(retrieved_user) = store.hget::<User>("users", "alice").await {
        println!("User: {:?}", retrieved_user);
    }
    
    // Check if field exists
    if store.hexists("users", "alice").await {
        println!("Alice exists in users hash");
    }
    
    // Get all users
    let all_users: std::collections::HashMap<String, User> = 
        store.hgetall("users").await;
    
    // Get hash statistics
    println!("Number of users: {}", store.hlen("users").await);
    println!("User keys: {:?}", store.hkeys("users").await);
}
```

## Running the Example

To see hash operations in action, run the included example:

```bash
cargo run --example hash_demo --features memory
```

This example demonstrates all hash operations including TTL functionality with detailed output showing how each operation works. The example includes:
- Basic hash operations (set, get, delete)
- Field existence checking
- Getting all keys, values, and field counts
- Per-field TTL demonstration with real-time expiration
- Separation between hash and regular key-value operations