#![cfg(feature = "memory")]
use store::{MemoryStore, Store};
use tokio::time::{Duration, sleep};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
struct TestData {
    value: String,
}

#[tokio::test]
async fn test_hash_field_ttl_expiration() {
    let store = MemoryStore::new();

    let data1 = TestData {
        value: "permanent".to_string(),
    };

    let data2 = TestData {
        value: "temporary".to_string(),
    };

    // Set a permanent field (no TTL)
    let _ = store.hset("test_hash", "permanent", &data1, None).await;

    // Set a temporary field (1 second TTL)
    let _ = store.hset("test_hash", "temporary", &data2, Some(1)).await;

    // Both fields should exist initially
    assert!(store.hexists("test_hash", "permanent").await.unwrap());
    assert!(store.hexists("test_hash", "temporary").await.unwrap());
    assert_eq!(store.hlen("test_hash").await.unwrap(), 2);

    // Both fields should be retrievable
    let retrieved_permanent: Option<TestData> = store.hget("test_hash", "permanent").await.unwrap();
    let retrieved_temporary: Option<TestData> = store.hget("test_hash", "temporary").await.unwrap();

    assert_eq!(retrieved_permanent, Some(data1.clone()));
    assert_eq!(retrieved_temporary, Some(data2.clone()));

    // Wait for 1.5 seconds (temporary field should expire)
    sleep(Duration::from_millis(1500)).await;

    // Permanent field should still exist, temporary should be gone
    assert!(store.hexists("test_hash", "permanent").await.unwrap());
    assert!(!store.hexists("test_hash", "temporary").await.unwrap());
    assert_eq!(store.hlen("test_hash").await.unwrap(), 1);

    // Only permanent field should be retrievable
    let retrieved_permanent: Option<TestData> = store.hget("test_hash", "permanent").await.unwrap();
    let retrieved_temporary: Option<TestData> = store.hget("test_hash", "temporary").await.unwrap();

    assert_eq!(retrieved_permanent, Some(data1));
    assert_eq!(retrieved_temporary, None);
}

#[tokio::test]
async fn test_hash_ttl_affects_all_operations() {
    let store = MemoryStore::new();

    let data1 = TestData {
        value: "data1".to_string(),
    };
    let data2 = TestData {
        value: "data2".to_string(),
    };
    let data3 = TestData {
        value: "data3".to_string(),
    };

    // Set fields with different TTLs
    let _ = store.hset("test_hash", "short", &data1, Some(1)).await; // 1 second
    let _ = store.hset("test_hash", "medium", &data2, Some(2)).await; // 2 seconds
    let _ = store.hset("test_hash", "permanent", &data3, None).await; // no expiration

    // All fields should be present initially
    assert_eq!(store.hlen("test_hash").await.unwrap(), 3);
    assert_eq!(store.hkeys("test_hash").await.unwrap().len(), 3);
    assert_eq!(store.hvals::<TestData>("test_hash").await.unwrap().len(), 3);

    let all_data = store.hgetall::<TestData>("test_hash").await.unwrap();
    assert_eq!(all_data.len(), 3);
    assert!(all_data.contains_key("short"));
    assert!(all_data.contains_key("medium"));
    assert!(all_data.contains_key("permanent"));

    // Wait 1.5 seconds (short should expire)
    sleep(Duration::from_millis(1500)).await;

    assert_eq!(store.hlen("test_hash").await.unwrap(), 2);
    assert_eq!(store.hkeys("test_hash").await.unwrap().len(), 2);
    assert_eq!(store.hvals::<TestData>("test_hash").await.unwrap().len(), 2);

    let all_data = store.hgetall::<TestData>("test_hash").await.unwrap();
    assert_eq!(all_data.len(), 2);
    assert!(!all_data.contains_key("short"));
    assert!(all_data.contains_key("medium"));
    assert!(all_data.contains_key("permanent"));

    // Wait another 1 second (medium should expire)
    sleep(Duration::from_millis(1000)).await;

    assert_eq!(store.hlen("test_hash").await.unwrap(), 1);
    assert_eq!(store.hkeys("test_hash").await.unwrap().len(), 1);
    assert_eq!(store.hvals::<TestData>("test_hash").await.unwrap().len(), 1);

    let all_data = store.hgetall::<TestData>("test_hash").await.unwrap();
    assert_eq!(all_data.len(), 1);
    assert!(!all_data.contains_key("short"));
    assert!(!all_data.contains_key("medium"));
    assert!(all_data.contains_key("permanent"));
    assert_eq!(all_data["permanent"], data3);
}

#[tokio::test]
async fn test_hash_field_update_preserves_ttl_behavior() {
    let store = MemoryStore::new();

    let data1 = TestData {
        value: "original".to_string(),
    };
    let data2 = TestData {
        value: "updated".to_string(),
    };

    // Set field with TTL
    let is_new = store
        .hset("test_hash", "field", &data1, Some(1))
        .await
        .unwrap();
    assert!(is_new);

    // Update field with new TTL
    let is_new = store
        .hset("test_hash", "field", &data2, Some(2))
        .await
        .unwrap();
    assert!(!is_new); // Should be false since field already existed

    // Field should have updated value
    let retrieved: Option<TestData> = store.hget("test_hash", "field").await.unwrap();
    assert_eq!(retrieved, Some(data2.clone()));

    // Wait 1.5 seconds (original TTL would have expired, but new TTL should keep it alive)
    sleep(Duration::from_millis(1500)).await;

    // Field should still exist due to new TTL
    assert!(store.hexists("test_hash", "field").await.unwrap());
    let retrieved: Option<TestData> = store.hget("test_hash", "field").await.unwrap();
    assert_eq!(retrieved, Some(data2));

    // Wait another 1 second (new TTL should expire)
    sleep(Duration::from_millis(1000)).await;

    // Field should now be expired
    assert!(!store.hexists("test_hash", "field").await.unwrap());
    let retrieved: Option<TestData> = store.hget("test_hash", "field").await.unwrap();
    assert_eq!(retrieved, None);
}

#[tokio::test]
async fn test_hash_cleaner_removes_expired_fields() {
    let store = MemoryStore::new();

    // Start the cleaner with 1 second interval
    store.run_cleaner(1);

    let data = TestData {
        value: "test".to_string(),
    };

    // Set a field that expires quickly
    store
        .hset("test_hash", "expires_soon", &data, Some(1))
        .await
        .unwrap();

    // Field should exist initially
    assert!(store.hexists("test_hash", "expires_soon").await.unwrap());

    // Wait for expiration + cleaner interval
    sleep(Duration::from_millis(2500)).await;

    // Field should be cleaned up
    assert!(!store.hexists("test_hash", "expires_soon").await.unwrap());
}

#[tokio::test]
async fn test_mixed_ttl_and_non_ttl_fields() {
    let store = MemoryStore::new();

    let data = TestData {
        value: "test".to_string(),
    };

    // Set mix of TTL and non-TTL fields
    store
        .hset("mixed_hash", "no_ttl_1", &data, None)
        .await
        .unwrap();
    store
        .hset("mixed_hash", "ttl_field", &data, Some(1))
        .await
        .unwrap();
    store
        .hset("mixed_hash", "no_ttl_2", &data, None)
        .await
        .unwrap();

    assert_eq!(store.hlen("mixed_hash").await.unwrap(), 3);

    // Wait for TTL field to expire
    sleep(Duration::from_millis(1500)).await;

    // Only non-TTL fields should remain
    assert_eq!(store.hlen("mixed_hash").await.unwrap(), 2);
    assert!(store.hexists("mixed_hash", "no_ttl_1").await.unwrap());
    assert!(!store.hexists("mixed_hash", "ttl_field").await.unwrap());
    assert!(store.hexists("mixed_hash", "no_ttl_2").await.unwrap());

    let keys = store.hkeys("mixed_hash").await.unwrap();
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&"no_ttl_1".to_string()));
    assert!(keys.contains(&"no_ttl_2".to_string()));
    assert!(!keys.contains(&"ttl_field".to_string()));
}
