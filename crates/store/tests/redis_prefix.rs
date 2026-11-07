#![cfg(feature = "redis")]

use store::{AtomicStore, RedisStore, Store, StreamEntry};

#[tokio::test]
#[ignore = "requires isolated REDIS_PREFIX_TEST_URL with the restricted Stargate ACL"]
async fn prefixed_state_and_explicit_stream() -> Result<(), Box<dyn std::error::Error>> {
    let store = RedisStore::new(&std::env::var("REDIS_PREFIX_TEST_URL")?)
        .await?
        .with_key_prefix("stargate:");
    assert_eq!(store.prefixed_key("account:test"), "stargate:account:test");
    assert_eq!(
        store.prefixed_key("stargate:test"),
        "stargate:stargate:test"
    );
    let peer = RedisStore::new(&std::env::var("REDIS_PREFIX_TEST_URL")?).await?;
    assert_eq!(peer.prefixed_key("legacy"), "legacy");
    store.ping().await?;
    store.set("value", &"first", Some(60)).await?;
    assert_eq!(
        peer.get::<String>("stargate:value").await?.as_deref(),
        Some("first")
    );
    assert!(store.exists("value").await?);
    assert!(!store.set_if_absent("value", &"other", None).await?);
    assert!(store.set_if_absent("absent", &"created", Some(60)).await?);
    assert!(
        store
            .compare_and_swap("value", &"first", &"second", Some(60))
            .await?
    );
    assert_eq!(
        store.get::<String>("value").await?.as_deref(),
        Some("second")
    );
    assert!(store.hset("hash", "field", &42, Some(60)).await?);
    assert_eq!(store.hget::<i32>("hash", "field").await?, Some(42));
    assert!(store.hexists("hash", "field").await?);
    assert_eq!(store.hkeys("hash").await?, vec!["field"]);
    assert_eq!(store.hvals::<i32>("hash").await?, vec![42]);
    assert_eq!(store.hgetall::<i32>("hash").await?["field"], 42);
    assert_eq!(store.hlen("hash").await?, 1);
    assert!(store.hset("hash", "persistent", &7, None).await?);
    assert!(store.hdel("hash", "field").await?);
    store.set_i64("integer", 10, None).await?;
    assert_eq!(store.get_i64("integer").await?, Some(10));
    assert_eq!(store.incr_i64("integer", 2, None).await?, Some(12));
    assert_eq!(store.decr_i64("integer", 1, None).await?, Some(11));
    assert_eq!(store.incr_i64("integer", 2, Some(60)).await?, Some(13));
    assert_eq!(store.decr_i64("integer", 1, Some(60)).await?, Some(12));
    assert!(
        store
            .compare_and_swap_i64("integer", 12, 99, Some(60))
            .await?
    );
    store.set_i64("integer", 1, Some(60)).await?;
    store
        .batch_set(&[("batch1", &1, None), ("batch2", &2, Some(60))])
        .await?;
    assert_eq!(
        store
            .batch_get::<i32>(&["batch1", "batch2", "missing"])
            .await?,
        vec![Some(1), Some(2), None]
    );
    assert_eq!(
        store.batch_exists(&["batch1", "missing"]).await?,
        vec![true, false]
    );
    assert_eq!(
        store.batch_delete(&["batch1", "batch2", "missing"]).await?,
        vec![true, true, false]
    );
    assert!(store.delete("value").await?);
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
