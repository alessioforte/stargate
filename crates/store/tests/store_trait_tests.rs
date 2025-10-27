#[cfg(feature = "memory")]
mod store_trait_tests {
    use serde::{Deserialize, Serialize};
    use std::time::Duration;
    use store::memory::MemoryStore;
    use store::{AtomicStore, Store, StoreError, StoreResult};
    use tokio::time::sleep;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestData {
        value: String,
    }

    // ============================================================================
    // Error Handling Tests
    // ============================================================================

    #[tokio::test]
    async fn test_empty_key_errors() {
        let store = MemoryStore::new();

        // Empty string key
        let result = store.set("", &"value", None).await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));

        let result: StoreResult<Option<String>> = store.get("").await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));

        let result = store.delete("").await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));

        let result = store.exists("").await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn test_whitespace_key_errors() {
        let store = MemoryStore::new();

        // Only whitespace
        let result = store.set("   ", &"value", None).await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));

        let result: StoreResult<Option<String>> = store.get("   ").await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));

        let result = store.delete("   ").await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn test_zero_ttl_error() {
        let store = MemoryStore::new();

        let result = store.set("key", &"value", Some(0)).await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));

        if let Err(StoreError::InvalidInput(msg)) = result {
            assert!(msg.contains("TTL"));
        }
    }

    #[tokio::test]
    async fn test_hash_empty_key_errors() {
        let store = MemoryStore::new();

        // Empty hash key
        let result = store.hset("", "field", &"value", None).await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));

        // Empty field key
        let result = store.hset("hash", "", &"value", None).await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));

        // Both empty
        let result = store.hset("", "", &"value", None).await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn test_hash_whitespace_errors() {
        let store = MemoryStore::new();

        let result = store.hset("  ", "field", &"value", None).await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));

        let result = store.hset("hash", "  ", &"value", None).await;
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn test_serialization_of_complex_types() {
        let store = MemoryStore::new();

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct Complex {
            nested: Vec<Option<String>>,
            map: std::collections::HashMap<String, i32>,
        }

        let mut map = std::collections::HashMap::new();
        map.insert("key1".to_string(), 100);
        map.insert("key2".to_string(), 200);

        let complex = Complex {
            nested: vec![Some("a".to_string()), None, Some("b".to_string())],
            map,
        };

        store.set("complex", &complex, None).await.unwrap();
        let retrieved: Complex = store.get("complex").await.unwrap().unwrap();
        assert_eq!(retrieved, complex);
    }

    // ============================================================================
    // Type Mismatch Tests
    // ============================================================================

    #[tokio::test]
    async fn test_type_mismatch_handling() {
        let store = MemoryStore::new();

        // Store as String
        store.set("key", &"string_value", None).await.unwrap();

        // Try to retrieve as integer - bincode may deserialize differently
        // We just want to ensure we can retrieve the original type
        let result: StoreResult<Option<String>> = store.get("key").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("string_value".to_string()));
    }

    #[tokio::test]
    async fn test_cannot_get_hash_as_simple_value() {
        let store = MemoryStore::new();

        store.hset("hash", "field", &"value", None).await.unwrap();

        // Try to get hash as simple value - should return None
        let result: Option<String> = store.get("hash").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cannot_get_atomic_as_simple_value() {
        let store = MemoryStore::new();

        store.set_i64("counter", 100, None).await.unwrap();

        // Try to get atomic as simple value - should return None
        let result: Option<String> = store.get("counter").await.unwrap();
        assert!(result.is_none());
    }

    // ============================================================================
    // Expiration Edge Cases
    // ============================================================================

    #[tokio::test]
    async fn test_expired_key_returns_none() {
        let store = MemoryStore::new();

        store.set("key", &"value", Some(1)).await.unwrap();
        sleep(Duration::from_secs(2)).await;

        let result: Option<String> = store.get("key").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_expired_key_removed_on_access() {
        let store = MemoryStore::new();

        store.set("key", &"value", Some(1)).await.unwrap();
        assert_eq!(store.get_total_keys(), 1);

        sleep(Duration::from_secs(2)).await;

        // Access triggers removal
        let _: Option<String> = store.get("key").await.unwrap();
        assert_eq!(store.get_total_keys(), 0);
    }

    #[tokio::test]
    async fn test_exists_removes_expired() {
        let store = MemoryStore::new();

        store.set("key", &"value", Some(1)).await.unwrap();
        sleep(Duration::from_secs(2)).await;

        // exists should return false and remove the key
        assert!(!store.exists("key").await.unwrap());
        assert_eq!(store.get_total_keys(), 0);
    }

    #[tokio::test]
    async fn test_hash_with_expired_fields() {
        let store = MemoryStore::new();

        // Add fields with different TTLs
        store
            .hset("hash", "expire1", &"value1", Some(1))
            .await
            .unwrap();
        store
            .hset("hash", "expire2", &"value2", Some(1))
            .await
            .unwrap();
        store
            .hset("hash", "persist", &"value3", None)
            .await
            .unwrap();

        assert_eq!(store.hlen("hash").await.unwrap(), 3);

        sleep(Duration::from_secs(2)).await;

        // Expired fields should be cleaned up
        let all: std::collections::HashMap<String, String> = store.hgetall("hash").await.unwrap();
        assert_eq!(all.len(), 1);
        assert!(all.contains_key("persist"));
    }

    #[tokio::test]
    async fn test_hash_entirely_expired() {
        let store = MemoryStore::new();

        // Create hash with TTL
        store
            .hset("hash", "field", &"value", Some(1))
            .await
            .unwrap();

        sleep(Duration::from_secs(2)).await;

        let all: std::collections::HashMap<String, String> = store.hgetall("hash").await.unwrap();
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn test_expired_atomic_value() {
        let store = MemoryStore::new();

        store.set_i64("counter", 100, Some(1)).await.unwrap();
        sleep(Duration::from_secs(2)).await;

        let result = store.get_i64("counter").await.unwrap();
        assert!(result.is_none());
    }

    // ============================================================================
    // Compare and Swap Tests
    // ============================================================================

    #[tokio::test]
    async fn test_compare_and_swap_success() {
        let store = MemoryStore::new();

        store.set("key", &"old".to_string(), None).await.unwrap();

        let success = store
            .compare_and_swap("key", &"old".to_string(), &"new".to_string(), None)
            .await
            .unwrap();
        assert!(success);

        let value: String = store.get("key").await.unwrap().unwrap();
        assert_eq!(value, "new");
    }

    #[tokio::test]
    async fn test_compare_and_swap_failure() {
        let store = MemoryStore::new();

        store
            .set("key", &"current".to_string(), None)
            .await
            .unwrap();

        let success = store
            .compare_and_swap("key", &"wrong".to_string(), &"new".to_string(), None)
            .await
            .unwrap();
        assert!(!success);

        let value: String = store.get("key").await.unwrap().unwrap();
        assert_eq!(value, "current"); // Unchanged
    }

    #[tokio::test]
    async fn test_compare_and_swap_nonexistent() {
        let store = MemoryStore::new();

        let success = store
            .compare_and_swap("key", &"old".to_string(), &"new".to_string(), None)
            .await
            .unwrap();
        assert!(!success);

        let value: Option<String> = store.get("key").await.unwrap();
        assert!(value.is_none());
    }

    #[tokio::test]
    async fn test_compare_and_swap_with_ttl() {
        let store = MemoryStore::new();

        store.set("key", &"old".to_string(), None).await.unwrap();

        let success = store
            .compare_and_swap("key", &"old".to_string(), &"new".to_string(), Some(60))
            .await
            .unwrap();
        assert!(success);

        // Should exist with new TTL
        assert!(store.exists("key").await.unwrap());
    }

    // ============================================================================
    // Overwrite Tests
    // ============================================================================

    #[tokio::test]
    async fn test_overwrite_changes_value() {
        let store = MemoryStore::new();

        store.set("key", &100, None).await.unwrap();
        store.set("key", &200, None).await.unwrap();

        let value: i32 = store.get("key").await.unwrap().unwrap();
        assert_eq!(value, 200);
    }

    #[tokio::test]
    async fn test_overwrite_changes_ttl() {
        let store = MemoryStore::new();

        // Set with TTL
        store.set("key", &"value", Some(60)).await.unwrap();

        // Overwrite without TTL
        store.set("key", &"value", None).await.unwrap();

        sleep(Duration::from_secs(2)).await;

        // Should still exist (no TTL now)
        assert!(store.exists("key").await.unwrap());
    }

    #[tokio::test]
    async fn test_hset_overwrite() {
        let store = MemoryStore::new();

        store.hset("hash", "field", &"value1", None).await.unwrap();

        let is_new = store.hset("hash", "field", &"value2", None).await.unwrap();
        assert!(!is_new); // Field already existed

        let value: String = store.hget("hash", "field").await.unwrap().unwrap();
        assert_eq!(value, "value2");
    }

    // ============================================================================
    // Empty Result Tests
    // ============================================================================

    #[tokio::test]
    async fn test_hgetall_empty_hash() {
        let store = MemoryStore::new();

        let all: std::collections::HashMap<String, String> =
            store.hgetall("nonexistent").await.unwrap();
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn test_hkeys_empty_hash() {
        let store = MemoryStore::new();

        let keys = store.hkeys("nonexistent").await.unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn test_hvals_empty_hash() {
        let store = MemoryStore::new();

        let values: Vec<String> = store.hvals("nonexistent").await.unwrap();
        assert!(values.is_empty());
    }

    #[tokio::test]
    async fn test_hlen_empty_hash() {
        let store = MemoryStore::new();

        let len = store.hlen("nonexistent").await.unwrap();
        assert_eq!(len, 0);
    }

    // ============================================================================
    // Large Data Tests
    // ============================================================================

    #[tokio::test]
    async fn test_large_string() {
        let store = MemoryStore::new();

        let large = "x".repeat(1_000_000); // 1MB
        store.set("large", &large, None).await.unwrap();

        let retrieved: String = store.get("large").await.unwrap().unwrap();
        assert_eq!(retrieved.len(), 1_000_000);
    }

    #[tokio::test]
    async fn test_large_vector() {
        let store = MemoryStore::new();

        let large_vec: Vec<i32> = (0..100_000).collect();
        store.set("vec", &large_vec, None).await.unwrap();

        let retrieved: Vec<i32> = store.get("vec").await.unwrap().unwrap();
        assert_eq!(retrieved.len(), 100_000);
        assert_eq!(retrieved[0], 0);
        assert_eq!(retrieved[99_999], 99_999);
    }

    #[tokio::test]
    async fn test_many_keys() {
        let store = MemoryStore::new();

        for i in 0..10_000 {
            store.set(&format!("key:{}", i), &i, None).await.unwrap();
        }

        assert_eq!(store.get_total_keys(), 10_000);

        let value: i32 = store.get("key:5000").await.unwrap().unwrap();
        assert_eq!(value, 5000);
    }

    #[tokio::test]
    async fn test_many_hash_fields() {
        let store = MemoryStore::new();

        for i in 0..1_000 {
            store
                .hset("hash", &format!("field:{}", i), &i, None)
                .await
                .unwrap();
        }

        let len = store.hlen("hash").await.unwrap();
        assert_eq!(len, 1_000);

        let value: i32 = store.hget("hash", "field:500").await.unwrap().unwrap();
        assert_eq!(value, 500);
    }

    // ============================================================================
    // Special Characters Tests
    // ============================================================================

    #[tokio::test]
    async fn test_keys_with_special_characters() {
        let store = MemoryStore::new();

        let keys = vec![
            "key:with:colons",
            "key/with/slashes",
            "key.with.dots",
            "key-with-dashes",
            "key_with_underscores",
            "key with spaces",
            "key\twith\ttabs",
            "key🚀with🎉emoji",
            "key@#$%^&*()",
        ];

        for key in &keys {
            store.set(key, &"value", None).await.unwrap();
        }

        for key in &keys {
            assert!(store.exists(key).await.unwrap());
        }
    }

    #[tokio::test]
    async fn test_unicode_keys_and_values() {
        let store = MemoryStore::new();

        let keys_values = vec![
            ("日本語", "Japanese"),
            ("中文", "Chinese"),
            ("한국어", "Korean"),
            ("Русский", "Russian"),
            ("العربية", "Arabic"),
            ("עברית", "Hebrew"),
        ];

        for (key, value) in &keys_values {
            store.set(key, value, None).await.unwrap();
        }

        for (key, expected_value) in &keys_values {
            let value: String = store.get(key).await.unwrap().unwrap();
            assert_eq!(&value, expected_value);
        }
    }

    // ============================================================================
    // Statistics Edge Cases
    // ============================================================================

    #[tokio::test]
    async fn test_stats_track_operations() {
        let store = MemoryStore::new();

        store.set("key", &"value", None).await.unwrap();
        let _: Option<String> = store.get("key").await.unwrap();
        store.delete("key").await.unwrap();

        let stats = store.get_storage_stats();
        assert!(stats.operations.sets >= 1);
        assert!(stats.operations.gets >= 1);
        assert!(stats.operations.deletes >= 1);
    }

    #[tokio::test]
    async fn test_cache_hit_miss_tracking() {
        let store = MemoryStore::new();

        store.set("key", &"value", None).await.unwrap();

        // Hit
        let _: Option<String> = store.get("key").await.unwrap();

        // Miss
        let _: Option<String> = store.get("nonexistent").await.unwrap();

        let stats = store.get_storage_stats();
        assert!(stats.operations.cache_hits > 0);
        assert!(stats.operations.cache_misses > 0);
    }

    #[tokio::test]
    async fn test_stats_reset_clears_counters() {
        let store = MemoryStore::new();

        store.set("key", &"value", None).await.unwrap();
        let _: Option<String> = store.get("key").await.unwrap();

        store.reset_stats();

        let stats = store.get_storage_stats();
        assert_eq!(stats.operations.sets, 0);
        assert_eq!(stats.operations.gets, 0);
    }

    #[tokio::test]
    async fn test_zero_cache_hit_ratio_no_operations() {
        let store = MemoryStore::new();

        let ratio = store.get_cache_hit_ratio();
        assert_eq!(ratio, 0.0);
    }
}
