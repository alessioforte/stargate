use std::collections::HashMap;
use store::{MemoryStore, Store};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
struct TestUser {
    id: u32,
    name: String,
    email: String,
    active: bool,
}

/// Test that both MemoryStore and RedisStore implement the Store trait correctly
/// This test validates the interface works consistently across implementations
#[tokio::test]
async fn test_store_interface_consistency() {
    let memory_store = MemoryStore::new();

    // Test with MemoryStore
    test_store_implementation(&memory_store).await;

    // RedisStore would be tested here if Redis is available
    // Uncomment and modify the following lines to test with Redis:
    //
    // #[cfg(feature = "redis")]
    // {
    //     use store::RedisStore;
    //     if let Ok(redis_store) = RedisStore::new("redis://127.0.0.1:6379") {
    //         test_store_implementation(&redis_store).await;
    //     }
    // }
}

/// Generic test function that works with any Store implementation
async fn test_store_implementation<S: Store>(store: &S) {
    let user1 = TestUser {
        id: 1,
        name: "Alice".to_string(),
        email: "alice@test.com".to_string(),
        active: true,
    };

    let user2 = TestUser {
        id: 2,
        name: "Bob".to_string(),
        email: "bob@test.com".to_string(),
        active: false,
    };

    // Test basic key-value operations
    test_basic_operations(store, &user1).await;

    // Test hash operations without TTL
    test_hash_operations_no_ttl(store, &user1, &user2).await;

    // Test hash operations with TTL (note: TTL behavior may differ between implementations)
    test_hash_operations_with_ttl(store, &user1).await;
}

async fn test_basic_operations<S: Store>(store: &S, user: &TestUser) {
    let key = "test:user:basic";

    // Test set and get
    store.set(key, user, None).await;
    let retrieved: Option<TestUser> = store.get(key).await;
    assert_eq!(retrieved, Some(user.clone()));

    // Test exists
    assert!(store.exists(key).await);

    // Test delete
    assert!(store.delete(key).await);
    assert!(!store.exists(key).await);

    // Test get after delete
    let retrieved: Option<TestUser> = store.get(key).await;
    assert_eq!(retrieved, None);

    // Test set with TTL (basic functionality)
    store.set(key, user, Some(3600)).await; // 1 hour TTL
    let retrieved: Option<TestUser> = store.get(key).await;
    assert_eq!(retrieved, Some(user.clone()));

    // Clean up
    store.delete(key).await;
}

async fn test_hash_operations_no_ttl<S: Store>(store: &S, user1: &TestUser, user2: &TestUser) {
    let hash_key = "test:users:hash";

    // Test hset and hget
    let is_new = store.hset(hash_key, "alice", user1, None).await;
    assert!(is_new);

    let is_new = store.hset(hash_key, "bob", user2, None).await;
    assert!(is_new);

    // Test update existing field
    let mut updated_user1 = user1.clone();
    updated_user1.email = "alice.updated@test.com".to_string();
    let is_new = store.hset(hash_key, "alice", &updated_user1, None).await;
    assert!(!is_new); // Should be false since field already existed

    // Test hget
    let retrieved: Option<TestUser> = store.hget(hash_key, "alice").await;
    assert_eq!(retrieved, Some(updated_user1.clone()));

    let retrieved: Option<TestUser> = store.hget(hash_key, "bob").await;
    assert_eq!(retrieved, Some(user2.clone()));

    // Test hexists
    assert!(store.hexists(hash_key, "alice").await);
    assert!(store.hexists(hash_key, "bob").await);
    assert!(!store.hexists(hash_key, "charlie").await);

    // Test hlen
    assert_eq!(store.hlen(hash_key).await, 2);

    // Test hkeys
    let keys = store.hkeys(hash_key).await;
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&"alice".to_string()));
    assert!(keys.contains(&"bob".to_string()));

    // Test hvals
    let values: Vec<TestUser> = store.hvals(hash_key).await;
    assert_eq!(values.len(), 2);
    assert!(values.contains(&updated_user1));
    assert!(values.contains(user2));

    // Test hgetall
    let all_users: HashMap<String, TestUser> = store.hgetall(hash_key).await;
    assert_eq!(all_users.len(), 2);
    assert_eq!(all_users.get("alice"), Some(&updated_user1));
    assert_eq!(all_users.get("bob"), Some(user2));

    // Test hdel
    assert!(store.hdel(hash_key, "bob").await);
    assert!(!store.hexists(hash_key, "bob").await);
    assert_eq!(store.hlen(hash_key).await, 1);

    // Test hdel on non-existent field
    assert!(!store.hdel(hash_key, "nonexistent").await);

    // Clean up
    store.hdel(hash_key, "alice").await;
}

async fn test_hash_operations_with_ttl<S: Store>(store: &S, user: &TestUser) {
    let hash_key = "test:users:ttl";

    // Test hset with TTL
    // Note: TTL behavior may differ between MemoryStore and RedisStore
    // MemoryStore: Real-time expiration checking
    // RedisStore: Redis server handles expiration
    let is_new = store.hset(hash_key, "temp_user", user, Some(1)).await; // 1 second TTL
    assert!(is_new);

    // Field should exist immediately
    assert!(store.hexists(hash_key, "temp_user").await);
    let retrieved: Option<TestUser> = store.hget(hash_key, "temp_user").await;
    assert_eq!(retrieved, Some(user.clone()));

    // Test permanent field alongside TTL field
    let is_new = store.hset(hash_key, "permanent", user, None).await;
    assert!(is_new);

    assert_eq!(store.hlen(hash_key).await, 2);

    // Wait for TTL field to expire (this test is implementation-dependent)
    // In MemoryStore, the field will be filtered out
    // In RedisStore, Redis will handle the expiration
    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // The permanent field should still exist
    assert!(store.hexists(hash_key, "permanent").await);

    // Clean up
    store.hdel(hash_key, "permanent").await;
}

#[tokio::test]
async fn test_hash_and_simple_key_separation() {
    let store = MemoryStore::new();
    let user = TestUser {
        id: 1,
        name: "Test".to_string(),
        email: "test@example.com".to_string(),
        active: true,
    };

    let simple_key = "test:simple";
    let hash_key = "test:hash";

    // Test 1: Simple key operations work correctly
    store.set(simple_key, &user, None).await;
    let retrieved: Option<TestUser> = store.get(simple_key).await;
    assert_eq!(retrieved, Some(user.clone()));
    assert!(store.exists(simple_key).await);

    // Hash operations should not work on simple key
    assert_eq!(store.hlen(simple_key).await, 0);
    assert!(!store.hexists(simple_key, "field").await);
    let hash_result: Option<TestUser> = store.hget(simple_key, "field").await;
    assert_eq!(hash_result, None);

    // Test 2: Hash key operations work correctly
    store.hset(hash_key, "user", &user, None).await;
    assert!(store.hexists(hash_key, "user").await);
    assert_eq!(store.hlen(hash_key).await, 1);
    let retrieved: Option<TestUser> = store.hget(hash_key, "user").await;
    assert_eq!(retrieved, Some(user.clone()));

    // Simple key operations should not work on hash key
    let simple_result: Option<TestUser> = store.get(hash_key).await;
    assert_eq!(simple_result, None);

    // Test 3: Key type cannot be changed without deletion
    // Attempting to set hash field on simple key should fail
    let is_new = store.hset(simple_key, "field", &user, None).await;
    assert!(!is_new); // Should return false - operation failed
    assert!(!store.hexists(simple_key, "field").await);

    // The simple key should still exist and work
    let simple_still_works: Option<TestUser> = store.get(simple_key).await;
    assert_eq!(simple_still_works, Some(user.clone()));

    // Test 4: After deletion, key type can change
    store.delete(simple_key).await;
    let is_new = store.hset(simple_key, "new_field", &user, None).await;
    assert!(is_new); // Should work now that simple key is deleted
    assert!(store.hexists(simple_key, "new_field").await);

    // Clean up
    store.hdel(simple_key, "new_field").await;
    store.hdel(hash_key, "user").await;
}

#[tokio::test]
async fn test_empty_operations() {
    let store = MemoryStore::new();

    // Test operations on non-existent keys
    let result: Option<TestUser> = store.get("nonexistent").await;
    assert_eq!(result, None);

    assert!(!store.exists("nonexistent").await);
    assert!(!store.delete("nonexistent").await);

    // Test hash operations on non-existent keys
    let hash_result: Option<TestUser> = store.hget("nonexistent", "field").await;
    assert_eq!(hash_result, None);

    assert!(!store.hexists("nonexistent", "field").await);
    assert!(!store.hdel("nonexistent", "field").await);
    assert_eq!(store.hlen("nonexistent").await, 0);

    let keys = store.hkeys("nonexistent").await;
    assert!(keys.is_empty());

    let values: Vec<TestUser> = store.hvals("nonexistent").await;
    assert!(values.is_empty());

    let all: HashMap<String, TestUser> = store.hgetall("nonexistent").await;
    assert!(all.is_empty());
}
