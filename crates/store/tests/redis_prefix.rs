#![cfg(feature = "redis")]

use std::time::Duration;
use store::{AtomicStore, RedisStore, Store, StoreError, StreamEntry, keys, with_request_budget};

const PREFIX: &str = "tenant:";

fn test_tag() -> String {
    format!("test:{}", ulid::Ulid::generate())
}

fn key(tag: &str, name: &str) -> String {
    keys::scoped(tag, name)
}

#[tokio::test]
#[ignore = "requires isolated REDIS_PREFIX_TEST_URL with the restricted Stargate ACL"]
async fn prefixed_state_and_explicit_stream() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("REDIS_PREFIX_TEST_URL")?;
    let store = RedisStore::new(&url).await?.with_key_prefix(PREFIX);
    let tag = test_tag();
    let value = key(&tag, "value");
    assert_eq!(
        store.prefixed_key(&value),
        format!("{PREFIX}{value}"),
        "the operator prefix precedes the schema root"
    );
    assert_eq!(store.prefixed_key("legacy"), "tenant:legacy");
    // A reader without the operator prefix addresses the physical key.
    let peer = RedisStore::new(&url).await?;
    assert_eq!(peer.prefixed_key("legacy"), "legacy");
    store.ping().await?;
    store.set(&value, &"first", Some(60)).await?;
    assert_eq!(
        peer.get::<String>(&format!("{PREFIX}{value}"))
            .await?
            .as_deref(),
        Some("first")
    );
    assert!(store.exists(&value).await?);
    assert!(!store.set_if_absent(&value, &"other", None).await?);
    assert!(
        store
            .set_if_absent(&key(&tag, "absent"), &"created", Some(60))
            .await?
    );
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
    assert!(store.hexists(&hash, "field").await?);
    assert_eq!(store.hkeys(&hash).await?, vec!["field"]);
    assert_eq!(store.hvals::<i32>(&hash).await?, vec![42]);
    assert_eq!(store.hgetall::<i32>(&hash).await?["field"], 42);
    assert_eq!(store.hlen(&hash).await?, 1);
    assert!(store.hset(&hash, "persistent", &7, None).await?);
    assert!(store.hdel(&hash, "field").await?);

    let integer = key(&tag, "integer");
    store.set_i64(&integer, 10, None).await?;
    assert_eq!(store.get_i64(&integer).await?, Some(10));
    assert_eq!(store.incr_i64(&integer, 2, None).await?, Some(12));
    assert_eq!(store.decr_i64(&integer, 1, None).await?, Some(11));
    assert_eq!(store.incr_i64(&integer, 2, Some(60)).await?, Some(13));
    assert_eq!(store.decr_i64(&integer, 1, Some(60)).await?, Some(12));
    assert!(
        store
            .compare_and_swap_i64(&integer, 12, 99, Some(60))
            .await?
    );
    store.set_i64(&integer, 1, Some(60)).await?;

    let batch1 = key(&tag, "batch1");
    let batch2 = key(&tag, "batch2");
    store
        .batch_set(&[(batch1.as_str(), &1, None), (batch2.as_str(), &2, Some(60))])
        .await?;
    assert_eq!(
        store
            .batch_get::<i32>(&[&batch1, &batch2, "stargate:v1:{missing}:x"])
            .await?,
        vec![Some(1), Some(2), None]
    );
    assert_eq!(
        store
            .batch_exists(&[&batch1, "stargate:v1:{missing}:x"])
            .await?,
        vec![true, false]
    );
    // Two entity tags are two slots: the batch is split by tag, not sent as
    // one cross-slot command.
    let other = keys::scoped(&test_tag(), "batch2");
    assert_eq!(
        store.batch_get::<i32>(&[&batch1, &other]).await?,
        vec![Some(1), None]
    );
    assert_eq!(
        store
            .batch_delete(&[&batch1, &batch2, "stargate:v1:{missing}:x"])
            .await?,
        vec![true, true, false]
    );
    assert!(store.delete(&value).await?);

    // Streams keep their explicit, unnamespaced name.
    store
        .xadd_batch(
            "audit.raw",
            None,
            &[StreamEntry {
                fields: vec![("payload".into(), "{}".into())],
            }],
        )
        .await?;
    assert!(store.stream_length("audit.raw").await? > 0);
    let zero = "0-0".parse()?;
    assert!(
        store
            .stream_groups("audit.raw", &["redis-prefix-test".into()])
            .await?
            .is_some()
    );
    assert!(
        store
            .first_stream_id_after("audit.raw", zero)
            .await?
            .is_some()
    );
    assert_eq!(store.trim_stream_before("audit.raw", zero).await?, 0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated REDIS_PREFIX_TEST_URL with the restricted Stargate ACL"]
async fn nested_scope_suspends_and_restores_the_request_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("REDIS_PREFIX_TEST_URL")?;
    let store = RedisStore::new(&url).await?.with_key_prefix(PREFIX);
    let value = key(&test_tag(), "budget");
    store.set(&value, &42, Some(60)).await?;

    with_request_budget(Some(Duration::ZERO), async {
        assert!(matches!(
            store.get::<i32>(&value).await,
            Err(StoreError::Timeout(_))
        ));
        assert_eq!(
            with_request_budget(None, store.get::<i32>(&value)).await?,
            Some(42)
        );
        assert!(matches!(
            store.get::<i32>(&value).await,
            Err(StoreError::Timeout(_))
        ));
        Ok::<(), StoreError>(())
    })
    .await?;
    Ok(())
}
