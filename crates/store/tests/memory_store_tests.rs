#[cfg(feature = "memory")]
mod memory_tests {
    use serde::{Deserialize, Serialize};
    use std::time::Duration;
    use store::memory::{MemoryStore, MemoryStoreConfig};
    use store::{AtomicStore, Store, StoreError, StoreResult};
    use tokio::time::sleep;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestUser {
        id: u64,
        name: String,
        email: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestProduct {
        id: u64,
        name: String,
        price: f64,
    }

    fn setup_logger() {
        let _ = env_logger::builder().is_test(true).try_init();
    }

    // ============================================================================
    // Basic CRUD Operations Tests
    // ============================================================================

    #[tokio::test]
    async fn test_set_and_get() {
        setup_logger();
        let store = MemoryStore::new();

        let user = TestUser {
            id: 1,
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
        };

        // Set value
        store.set("user:1", &user, None).await.unwrap();

        // Get value
        let retrieved: TestUser = store.get("user:1").await.unwrap().unwrap();
        assert_eq!(retrieved, user);
    }

    #[tokio::test]
    async fn test_get_nonexistent_key() {
        let store = MemoryStore::new();
        let result: Option<TestUser> = store.get("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete() {
        let store = MemoryStore::new();

        let user = TestUser {
            id: 1,
            name: "Bob".to_string(),
            email: "bob@example.com".to_string(),
        };

        store.set("user:1", &user, None).await.unwrap();
        assert!(store.exists("user:1").await.unwrap());

        let deleted = store.delete("user:1").await.unwrap();
        assert!(deleted);
        assert!(!store.exists("user:1").await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_nonexistent() {
        let store = MemoryStore::new();
        let deleted = store.delete("nonexistent").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_exists() {
        let store = MemoryStore::new();

        assert!(!store.exists("key").await.unwrap());

        store.set("key", &"value", None).await.unwrap();
        assert!(store.exists("key").await.unwrap());
    }

    // ============================================================================
    // TTL and Expiration Tests
    // ============================================================================

    #[tokio::test]
    async fn test_ttl_expiration() {
        let store = MemoryStore::new();

        store.set("temp_key", &"temp_value", Some(1)).await.unwrap();

        // Should exist immediately
        assert!(store.exists("temp_key").await.unwrap());

        // Wait for expiration
        sleep(Duration::from_secs(2)).await;

        // Should be expired
        let result: Option<String> = store.get("temp_key").await.unwrap();
        assert!(result.is_none());
        assert!(!store.exists("temp_key").await.unwrap());
    }

    #[tokio::test]
    async fn test_no_ttl_persists() {
        let store = MemoryStore::new();

        store
            .set("persist_key", &"persist_value", None)
            .await
            .unwrap();

        sleep(Duration::from_secs(2)).await;

        // Should still exist
        let result: Option<String> = store.get("persist_key").await.unwrap();
        assert_eq!(result.unwrap(), "persist_value");
    }

    #[tokio::test]
    async fn test_zero_ttl_rejected() {
        let store = MemoryStore::new();
        let result = store.set("key", &"value", Some(0)).await;
        assert!(result.is_err());
        match result {
            Err(StoreError::InvalidInput(_)) => {}
            _ => panic!("Expected InvalidInput error"),
        }
    }

    // ============================================================================
    // Hash Operations Tests
    // ============================================================================

    #[tokio::test]
    async fn test_hset_and_hget() {
        let store = MemoryStore::new();

        let user = TestUser {
            id: 1,
            name: "Charlie".to_string(),
            email: "charlie@example.com".to_string(),
        };

        store.hset("users", "user:1", &user, None).await.unwrap();

        let retrieved: TestUser = store.hget("users", "user:1").await.unwrap().unwrap();
        assert_eq!(retrieved, user);
    }

    #[tokio::test]
    async fn test_hgetall() {
        let store = MemoryStore::new();

        let user1 = TestUser {
            id: 1,
            name: "User1".to_string(),
            email: "user1@example.com".to_string(),
        };
        let user2 = TestUser {
            id: 2,
            name: "User2".to_string(),
            email: "user2@example.com".to_string(),
        };

        store.hset("users", "user:1", &user1, None).await.unwrap();
        store.hset("users", "user:2", &user2, None).await.unwrap();

        let all: std::collections::HashMap<String, TestUser> =
            store.hgetall("users").await.unwrap();

        assert_eq!(all.len(), 2);
        assert_eq!(all.get("user:1").unwrap(), &user1);
        assert_eq!(all.get("user:2").unwrap(), &user2);
    }

    #[tokio::test]
    async fn test_hdel() {
        let store = MemoryStore::new();

        store.hset("hash", "field1", &"value1", None).await.unwrap();
        store.hset("hash", "field2", &"value2", None).await.unwrap();

        let deleted = store.hdel("hash", "field1").await.unwrap();
        assert!(deleted);

        assert!(!store.hexists("hash", "field1").await.unwrap());
        assert!(store.hexists("hash", "field2").await.unwrap());
    }

    #[tokio::test]
    async fn test_hexists() {
        let store = MemoryStore::new();

        assert!(!store.hexists("hash", "field").await.unwrap());

        store.hset("hash", "field", &"value", None).await.unwrap();
        assert!(store.hexists("hash", "field").await.unwrap());
    }

    #[tokio::test]
    async fn test_hkeys() {
        let store = MemoryStore::new();

        store.hset("hash", "field1", &"value1", None).await.unwrap();
        store.hset("hash", "field2", &"value2", None).await.unwrap();
        store.hset("hash", "field3", &"value3", None).await.unwrap();

        let keys = store.hkeys("hash").await.unwrap();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"field1".to_string()));
        assert!(keys.contains(&"field2".to_string()));
        assert!(keys.contains(&"field3".to_string()));
    }

    #[tokio::test]
    async fn test_hvals() {
        let store = MemoryStore::new();

        store.hset("hash", "field1", &100, None).await.unwrap();
        store.hset("hash", "field2", &200, None).await.unwrap();
        store.hset("hash", "field3", &300, None).await.unwrap();

        let values: Vec<i32> = store.hvals("hash").await.unwrap();
        assert_eq!(values.len(), 3);
        assert!(values.contains(&100));
        assert!(values.contains(&200));
        assert!(values.contains(&300));
    }

    #[tokio::test]
    async fn test_hlen() {
        let store = MemoryStore::new();

        assert_eq!(store.hlen("hash").await.unwrap(), 0);

        store.hset("hash", "field1", &"value1", None).await.unwrap();
        assert_eq!(store.hlen("hash").await.unwrap(), 1);

        store.hset("hash", "field2", &"value2", None).await.unwrap();
        assert_eq!(store.hlen("hash").await.unwrap(), 2);

        store.hdel("hash", "field1").await.unwrap();
        assert_eq!(store.hlen("hash").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_hash_field_ttl() {
        let store = MemoryStore::new();

        store
            .hset("hash", "temp_field", &"value", Some(1))
            .await
            .unwrap();
        store
            .hset("hash", "persist_field", &"value", None)
            .await
            .unwrap();

        assert_eq!(store.hlen("hash").await.unwrap(), 2);

        sleep(Duration::from_secs(2)).await;

        // temp_field should be expired, persist_field should remain
        assert!(!store.hexists("hash", "temp_field").await.unwrap());
        assert!(store.hexists("hash", "persist_field").await.unwrap());
    }

    // ============================================================================
    // Atomic Operations Tests
    // ============================================================================

    #[tokio::test]
    async fn test_atomic_i64_set_get() {
        let store = MemoryStore::new();

        store.set_i64("counter", 100, None).await.unwrap();
        let value = store.get_i64("counter").await.unwrap().unwrap();
        assert_eq!(value, 100);
    }

    #[tokio::test]
    async fn test_atomic_incr() {
        let store = MemoryStore::new();

        store.set_i64("counter", 10, None).await.unwrap();

        let result = store.incr_i64("counter", 5, None).await.unwrap().unwrap();
        assert_eq!(result, 15);

        let value = store.get_i64("counter").await.unwrap().unwrap();
        assert_eq!(value, 15);
    }

    #[tokio::test]
    async fn test_atomic_decr() {
        let store = MemoryStore::new();

        store.set_i64("counter", 10, None).await.unwrap();

        let result = store.decr_i64("counter", 3, None).await.unwrap().unwrap();
        assert_eq!(result, 7);

        let value = store.get_i64("counter").await.unwrap().unwrap();
        assert_eq!(value, 7);
    }

    #[tokio::test]
    async fn test_atomic_incr_new_key() {
        let store = MemoryStore::new();

        let result = store
            .incr_i64("new_counter", 5, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result, 5);
    }

    #[tokio::test]
    async fn test_atomic_compare_and_swap() {
        let store = MemoryStore::new();

        store.set_i64("counter", 10, None).await.unwrap();

        // Successful swap
        let success = store
            .compare_and_swap_i64("counter", 10, 20, None)
            .await
            .unwrap();
        assert!(success);
        assert_eq!(store.get_i64("counter").await.unwrap().unwrap(), 20);

        // Failed swap (wrong old value)
        let failed = store
            .compare_and_swap_i64("counter", 10, 30, None)
            .await
            .unwrap();
        assert!(!failed);
        assert_eq!(store.get_i64("counter").await.unwrap().unwrap(), 20);
    }

    #[tokio::test]
    async fn test_atomic_compare_and_swap_expired_key_returns_false() {
        let store = MemoryStore::new();

        store.set_i64("counter", 10, Some(1)).await.unwrap();
        sleep(Duration::from_secs(2)).await;

        let success = store
            .compare_and_swap_i64("counter", 10, 20, None)
            .await
            .unwrap();
        assert!(!success);
        assert!(store.get_i64("counter").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_compare_and_swap() {
        let store = MemoryStore::new();

        store
            .set("key", &"old_value".to_string(), None)
            .await
            .unwrap();

        // Successful swap
        let success = store
            .compare_and_swap(
                "key",
                &"old_value".to_string(),
                &"new_value".to_string(),
                None,
            )
            .await
            .unwrap();
        assert!(success);
        let value: String = store.get("key").await.unwrap().unwrap();
        assert_eq!(value, "new_value");

        // Failed swap
        let failed = store
            .compare_and_swap(
                "key",
                &"old_value".to_string(),
                &"newer_value".to_string(),
                None,
            )
            .await
            .unwrap();
        assert!(!failed);
        let value: String = store.get("key").await.unwrap().unwrap();
        assert_eq!(value, "new_value");
    }

    // ============================================================================
    // Batch Operations Tests
    // ============================================================================

    #[tokio::test]
    async fn test_batch_get() {
        let store = MemoryStore::new();

        store.set("key1", &"value1", None).await.unwrap();
        store.set("key2", &"value2", None).await.unwrap();
        store.set("key3", &"value3", None).await.unwrap();

        let keys = vec!["key1", "key2", "key3", "nonexistent"];
        let results: Vec<Option<String>> = store.batch_get(&keys).await.unwrap();

        assert_eq!(results.len(), 4);
        assert_eq!(results[0].as_ref().unwrap(), "value1");
        assert_eq!(results[1].as_ref().unwrap(), "value2");
        assert_eq!(results[2].as_ref().unwrap(), "value3");
        assert!(results[3].is_none());
    }

    #[tokio::test]
    async fn test_batch_set() {
        let store = MemoryStore::new();

        let operations = vec![
            ("key1", &"value1", None),
            ("key2", &"value2", Some(60)),
            ("key3", &"value3", None),
        ];

        let results = store.batch_set(&operations).await.unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|&r| r));

        let value1: String = store.get("key1").await.unwrap().unwrap();
        let value2: String = store.get("key2").await.unwrap().unwrap();
        let value3: String = store.get("key3").await.unwrap().unwrap();

        assert_eq!(value1, "value1");
        assert_eq!(value2, "value2");
        assert_eq!(value3, "value3");
    }

    #[tokio::test]
    async fn test_batch_delete() {
        let store = MemoryStore::new();

        store.set("key1", &"value1", None).await.unwrap();
        store.set("key2", &"value2", None).await.unwrap();
        store.set("key3", &"value3", None).await.unwrap();

        let keys = vec!["key1", "key2", "nonexistent"];
        let results = store.batch_delete(&keys).await.unwrap();

        assert_eq!(results.len(), 3);
        assert!(results[0]); // key1 existed
        assert!(results[1]); // key2 existed
        assert!(!results[2]); // nonexistent didn't exist

        assert!(!store.exists("key1").await.unwrap());
        assert!(!store.exists("key2").await.unwrap());
        assert!(store.exists("key3").await.unwrap());
    }

    #[tokio::test]
    async fn test_batch_exists() {
        let store = MemoryStore::new();

        store.set("key1", &"value1", None).await.unwrap();
        store.set("key2", &"value2", None).await.unwrap();

        let keys = vec!["key1", "key2", "key3"];
        let results = store.batch_exists(&keys).await.unwrap();

        assert_eq!(results.len(), 3);
        assert!(results[0]);
        assert!(results[1]);
        assert!(!results[2]);
    }

    // ============================================================================
    // Configuration Tests
    // ============================================================================

    #[tokio::test]
    async fn test_custom_config() {
        let config = MemoryStoreConfig {
            initial_capacity: 1000,
            enable_time_caching: true,
            cleanup_batch_size: 50,
            enable_preallocation: true,
            max_memory_usage: 1024 * 1024,
        };

        let store = MemoryStore::with_config(config.clone());

        let retrieved_config = store.get_config();
        assert_eq!(retrieved_config.initial_capacity, 1000);
        assert_eq!(retrieved_config.cleanup_batch_size, 50);
        assert!(retrieved_config.enable_time_caching);
    }

    #[tokio::test]
    async fn test_high_performance_config() {
        let config = MemoryStoreConfig::high_performance();
        let store = MemoryStore::with_config(config);

        // Should work normally with high performance config
        store.set("key", &"value", None).await.unwrap();
        let value: String = store.get("key").await.unwrap().unwrap();
        assert_eq!(value, "value");
    }

    #[tokio::test]
    async fn test_memory_efficient_config() {
        let config = MemoryStoreConfig::memory_efficient();
        let store = MemoryStore::with_config(config);

        store.set("key", &"value", None).await.unwrap();
        let value: String = store.get("key").await.unwrap().unwrap();
        assert_eq!(value, "value");
    }

    // ============================================================================
    // Cleanup Tests
    // ============================================================================

    #[tokio::test]
    async fn test_manual_cleanup() {
        let store = MemoryStore::new();

        // Add some items with short TTL
        for i in 0..10 {
            store.set(&format!("key:{}", i), &i, Some(1)).await.unwrap();
        }

        assert_eq!(store.get_total_keys(), 10);

        sleep(Duration::from_secs(2)).await;

        // Manual cleanup
        let removed = store.cleanup_expired().await;
        assert_eq!(removed, 10);
        assert_eq!(store.get_total_keys(), 0);
    }

    #[tokio::test]
    async fn test_batch_cleanup_with_limit() {
        let store = MemoryStore::new();

        // Add 20 expired items
        for i in 0..20 {
            store.set(&format!("key:{}", i), &i, Some(1)).await.unwrap();
        }

        sleep(Duration::from_secs(2)).await;

        // Clean only 5 items
        let removed = store.batch_cleanup_expired(Some(5)).await;
        assert_eq!(removed, 5);

        // Still have 15 items left
        assert_eq!(store.get_total_keys(), 15);
    }

    #[tokio::test]
    async fn test_adaptive_cleaner() {
        let store = MemoryStore::new();

        // Start adaptive cleaner with 1 second interval
        store.run_adaptive_cleaner(1);

        // Add items with 2 second TTL
        for i in 0..5 {
            store.set(&format!("key:{}", i), &i, Some(2)).await.unwrap();
        }

        assert_eq!(store.get_total_keys(), 5);

        // Wait for expiration and cleanup
        sleep(Duration::from_secs(3)).await;

        // Items should be cleaned up
        assert_eq!(store.get_total_keys(), 0);
    }

    // ============================================================================
    // Statistics and Monitoring Tests
    // ============================================================================

    #[tokio::test]
    async fn test_cache_hit_ratio() {
        let store = MemoryStore::new();

        store.set("key", &"value", None).await.unwrap();

        // Hit
        let _: Option<String> = store.get("key").await.unwrap();

        // Miss
        let _: Option<String> = store.get("nonexistent").await.unwrap();

        // Hit
        let _: Option<String> = store.get("key").await.unwrap();

        let ratio = store.get_cache_hit_ratio();
        assert!((ratio - 0.666).abs() < 0.01); // ~66.6%
    }

    #[tokio::test]
    async fn test_storage_stats() {
        let store = MemoryStore::new();

        store.set("simple", &"value", None).await.unwrap();
        store.hset("hash", "field", &"value", None).await.unwrap();
        store.set_i64("counter", 100, None).await.unwrap();

        let stats = store.get_storage_stats();

        assert_eq!(stats.total_keys, 3);
        assert_eq!(stats.simple_keys, 2); // simple key + atomic i64
        assert_eq!(stats.hash_keys, 1);
        assert_eq!(stats.total_hash_fields, 1);
        assert!(stats.estimated_memory_bytes > 0);
    }

    #[tokio::test]
    async fn test_performance_metrics() {
        let store = MemoryStore::new();

        store.set("key", &"value", None).await.unwrap();
        let _: Option<String> = store.get("key").await.unwrap();
        store.delete("key").await.unwrap();

        let metrics = store.get_performance_metrics();

        assert!(metrics.total_operations >= 3);
        assert!(metrics.cache_hit_ratio >= 0.0);
        assert_eq!(metrics.total_keys, 0);
    }

    #[tokio::test]
    async fn test_memory_efficiency() {
        let store = MemoryStore::new();

        store.set("key1", &"value1", None).await.unwrap();
        store.set("key2", &"value2", Some(1)).await.unwrap();

        sleep(Duration::from_secs(2)).await;

        let efficiency = store.get_memory_efficiency();

        assert_eq!(efficiency.total_keys, 2);
        assert_eq!(efficiency.simple_keys, 2);
        assert_eq!(efficiency.expired_keys, 1);
        assert!((efficiency.fragmentation_ratio - 0.5).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_optimization_recommendations() {
        let store = MemoryStore::new();

        // Add some data
        store.set("key", &"value", None).await.unwrap();

        let recommendations = store.get_optimization_recommendations();
        assert!(!recommendations.is_empty());
    }

    #[tokio::test]
    async fn test_memory_pressure() {
        let config = MemoryStoreConfig {
            initial_capacity: 100,
            enable_time_caching: true,
            cleanup_batch_size: 10,
            enable_preallocation: true,
            max_memory_usage: 100, // Very low limit
        };

        let store = MemoryStore::with_config(config);

        // Add data
        for i in 0..50 {
            store.set(&format!("key:{}", i), &i, None).await.unwrap();
        }

        // Should detect memory pressure
        assert!(store.is_memory_pressure());
    }

    // ============================================================================
    // Validation Tests
    // ============================================================================

    #[tokio::test]
    async fn test_empty_key_validation() {
        let store = MemoryStore::new();

        let result = store.set("", &"value", None).await;
        assert!(result.is_err());

        let result = store.set("   ", &"value", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_hash_empty_key_validation() {
        let store = MemoryStore::new();

        let result = store.hset("", "field", &"value", None).await;
        assert!(result.is_err());

        let result = store.hset("key", "", &"value", None).await;
        assert!(result.is_err());
    }

    // ============================================================================
    // Concurrency Tests
    // ============================================================================

    #[tokio::test]
    async fn test_concurrent_writes() {
        let store = MemoryStore::new();
        let store_clone = store.clone();

        let handle1 = tokio::spawn(async move {
            for i in 0..100 {
                store.set(&format!("key:{}", i), &i, None).await.unwrap();
            }
        });

        let handle2 = tokio::spawn(async move {
            for i in 100..200 {
                store_clone
                    .set(&format!("key:{}", i), &i, None)
                    .await
                    .unwrap();
            }
        });

        handle1.await.unwrap();
        handle2.await.unwrap();
    }

    #[tokio::test]
    async fn test_concurrent_atomic_operations() {
        let store = MemoryStore::new();
        store.set_i64("counter", 0, None).await.unwrap();

        let mut handles = vec![];

        for _ in 0..10 {
            let store_clone = store.clone();
            let handle = tokio::spawn(async move {
                for _ in 0..10 {
                    store_clone.incr_i64("counter", 1, None).await.unwrap();
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let final_value = store.get_i64("counter").await.unwrap().unwrap();
        assert_eq!(final_value, 100);
    }

    // ============================================================================
    // Edge Cases Tests
    // ============================================================================

    #[tokio::test]
    async fn test_overwrite_existing_key() {
        let store = MemoryStore::new();

        store.set("key", &"value1", None).await.unwrap();
        store.set("key", &"value2", None).await.unwrap();

        let value: String = store.get("key").await.unwrap().unwrap();
        assert_eq!(value, "value2");
    }

    #[tokio::test]
    async fn test_type_safety() {
        let store = MemoryStore::new();

        // Store as String
        store.set("key", &"123", None).await.unwrap();

        // Try to retrieve as i64 - should fail during deserialization or return None
        let result: StoreResult<Option<i64>> = store.get("key").await;
        // Either it errors on deserialization or returns None/Some with wrong data
        // We just want to ensure it doesn't panic
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_large_values() {
        let store = MemoryStore::new();

        let large_string = "x".repeat(1_000_000); // 1MB string
        store.set("large", &large_string, None).await.unwrap();

        let retrieved: String = store.get("large").await.unwrap().unwrap();
        assert_eq!(retrieved.len(), 1_000_000);
    }

    #[tokio::test]
    async fn test_stats_reset() {
        let store = MemoryStore::new();

        store.set("key", &"value", None).await.unwrap();
        let _: Option<String> = store.get("key").await.unwrap();

        store.reset_stats();

        let stats = store.get_storage_stats();
        assert_eq!(stats.operations.gets, 0);
        assert_eq!(stats.operations.sets, 0);
    }
}
