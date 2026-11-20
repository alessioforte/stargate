#![cfg(feature = "redis")]

//! Store operations against a Redis Cluster node.
//!
//! The plain Redis client cannot follow `MOVED`, so the test connects to the
//! node that owns `REDIS_CLUSTER_TAG` and only uses keys under that tag. Every
//! key of one entity shares the tag, so an operation that works on a single
//! instance must work here without `CROSSSLOT` or `non local key` errors.

use store::{AtomicStore, RedisStore, Store, keys};

fn setup() -> (String, String, String) {
    (
        std::env::var("REDIS_CLUSTER_URL").expect("REDIS_CLUSTER_URL"),
        std::env::var("REDIS_CLUSTER_TAG").expect("REDIS_CLUSTER_TAG"),
        std::env::var("REDIS_CLUSTER_TAG_ALT").expect("REDIS_CLUSTER_TAG_ALT"),
    )
}

fn key(tag: &str, name: &str) -> String {
    keys::scoped(tag, name)
}

#[tokio::test]
#[ignore = "requires a Redis Cluster node that owns REDIS_CLUSTER_TAG"]
async fn one_entity_slot_supports_every_state_operation() -> Result<(), Box<dyn std::error::Error>>
{
    let (url, tag, alt_tag) = setup();
    let store = RedisStore::new(&url).await?.with_key_prefix("tenant:");

    let value = key(&tag, "value");
    store.set(&value, &"first", Some(60)).await?;
    assert_eq!(store.get::<String>(&value).await?.as_deref(), Some("first"));
    assert!(store.exists(&value).await?);
    assert!(!store.set_if_absent(&value, &"other", None).await?);
    assert!(
        store
            .compare_and_swap(&value, &"first", &"second", Some(60))
            .await?
    );
    assert_eq!(
        store.get::<String>(&value).await?.as_deref(),
        Some("second")
    );

    let hash = key(&tag, "hash");
    assert!(store.hset(&hash, "field", &42, Some(60)).await?);
    assert_eq!(store.hget::<i32>(&hash, "field").await?, Some(42));
    assert_eq!(store.hgetall::<i32>(&hash).await?["field"], 42);
    assert!(store.hset(&hash, "persistent", &7, None).await?);
    assert!(store.hdel(&hash, "field").await?);

    let integer = key(&tag, "integer");
    store.set_i64(&integer, 10, None).await?;
    assert_eq!(store.incr_i64(&integer, 2, None).await?, Some(12));
    assert_eq!(store.decr_i64(&integer, 1, Some(60)).await?, Some(11));
    assert!(
        store
            .compare_and_swap_i64(&integer, 11, 99, Some(60))
            .await?
    );

    let batch1 = key(&tag, "batch1");
    let batch2 = key(&tag, "batch2");
    store
        .batch_set(&[(batch1.as_str(), &1, None), (batch2.as_str(), &2, Some(60))])
        .await?;
    let missing = key(&tag, "missing");
    assert_eq!(
        store
            .batch_get::<i32>(&[&batch1, &batch2, &missing])
            .await?,
        vec![Some(1), Some(2), None]
    );
    assert_eq!(
        store.batch_exists(&[&batch1, &batch2]).await?,
        vec![true, true]
    );
    assert_eq!(
        store.batch_delete(&[&batch1, &batch2]).await?,
        vec![true, true]
    );

    // Two local slots: a raw multi-key command is rejected by the node, so
    // the store's per-slot batching is what makes the call above safe.
    let alt_batch = key(&alt_tag, "batch");
    store
        .batch_set(&[(batch1.as_str(), &1, None), (alt_batch.as_str(), &2, None)])
        .await?;
    assert_eq!(
        store.batch_get::<i32>(&[&batch1, &alt_batch]).await?,
        vec![Some(1), Some(2)]
    );
    let mut connection = store.get_connection();
    let physical = |key: &str| format!("tenant:{key}");
    let cross_slot = redis::cmd("MGET")
        .arg(physical(&batch1))
        .arg(physical(&alt_batch))
        .query_async::<Vec<Option<Vec<u8>>>>(&mut connection)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        cross_slot.to_ascii_lowercase().contains("crossslot"),
        "{cross_slot}"
    );
    let script_cross_slot = redis::cmd("EVAL")
        .arg("return 1")
        .arg(2)
        .arg(physical(&batch1))
        .arg(physical(&alt_batch))
        .query_async::<i64>(&mut connection)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        script_cross_slot.to_ascii_lowercase().contains("crossslot"),
        "{script_cross_slot}"
    );
    assert!(
        store
            .batch_delete(&[&batch1, &alt_batch])
            .await?
            .iter()
            .all(|deleted| *deleted)
    );

    assert!(store.delete(&value).await?);
    Ok(())
}
