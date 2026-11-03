use super::relay::{RelayConfig, check_stream, relay_checked_once};
use super::status::{AuditStatus, PauseReason};
use store::{RedisStore, StreamEntry};

/// Exercise the same guard and publication path as the relay against dedicated services.
#[tokio::test]
#[ignore = "requires dedicated AUDIT_RELAY_TEST_DATABASE_URL and AUDIT_RELAY_TEST_REDIS_URL services"]
async fn protected_stream_survives_out_of_order_acknowledgements_and_two_groups()
-> anyhow::Result<()> {
    use db::{
        DbStore,
        ent::{
            CredentialType, Profile, TrustedAdminActor, TrustedAuditContext, TrustedAuditRequest,
        },
    };
    let redis_url = std::env::var("AUDIT_RELAY_TEST_REDIS_URL")?;
    let svc = db::service::init(&std::env::var("AUDIT_RELAY_TEST_DATABASE_URL")?).await?;
    svc.migrate().await?;
    let store = RedisStore::new(&redis_url).await?;
    let mut con = redis::Client::open(redis_url)?
        .get_multiplexed_async_connection()
        .await?;
    let stream = format!("audit.protected.{}", ulid::Ulid::generate());
    let config = RelayConfig {
        stream: stream.clone(),
        maxlen: Some(500),
        batch: 10,
        idle: std::time::Duration::from_millis(10),
        groups: vec!["archiver".into(), "replica".into()],
        retention: None,
    };
    let mut state = AuditStatus::default();
    assert_eq!(
        check_stream(&store, &config, &mut state).await?,
        Some(PauseReason::GroupMissing)
    );
    for group in &config.groups {
        let _: String = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(&stream)
            .arg(group)
            .arg("0")
            .arg("MKSTREAM")
            .query_async(&mut con)
            .await?;
    }
    let entry = StreamEntry::new(vec![("payload".into(), "x".repeat(256))]);
    let _: String = redis::cmd("XADD")
        .arg(&stream)
        .arg("1-0")
        .arg("payload")
        .arg(&entry.fields[0].1)
        .query_async(&mut con)
        .await?;
    assert_eq!(check_stream(&store, &config, &mut state).await?, None);
    // No pending entries yet: XRANGE finds the first undelivered event.
    let age = state.oldest_unconfirmed_age_secs.unwrap();
    assert!(age > chrono::Utc::now().timestamp() as f64 - 2.0);
    type ReadReply = Vec<(String, Vec<(String, Vec<(String, String)>)>)>;
    async fn read(
        con: &mut redis::aio::MultiplexedConnection,
        stream: &str,
        group: &str,
    ) -> Vec<String> {
        let reply: ReadReply = redis::cmd("XREADGROUP")
            .arg("GROUP")
            .arg(group)
            .arg("reader")
            .arg("STREAMS")
            .arg(stream)
            .arg(">")
            .query_async(con)
            .await
            .unwrap();
        reply
            .into_iter()
            .flat_map(|(_, rows)| rows.into_iter().map(|(id, _)| id))
            .collect()
    }
    let first = read(&mut con, &stream, "archiver").await.remove(0);
    for _ in 0..9 {
        store
            .xadd_batch(&stream, None, &vec![entry.clone(); 50])
            .await?;
        let ids = read(&mut con, &stream, "archiver").await;
        let _: u64 = redis::cmd("XACK")
            .arg(&stream)
            .arg("archiver")
            .arg(ids)
            .query_async(&mut con)
            .await?;
        check_stream(&store, &config, &mut state).await?;
    }
    // The second group has never read: it still protects the entire stream.
    assert_eq!(state.stream_length, Some(451));
    assert_eq!(
        check_stream(&store, &config, &mut state).await?,
        Some(PauseReason::ConsumerBacklog)
    );
    let all = read(&mut con, &stream, "replica").await;
    let _: u64 = redis::cmd("XACK")
        .arg(&stream)
        .arg("replica")
        .arg(all)
        .query_async(&mut con)
        .await?;
    // Even with the second group caught up, the first pending entry pins the boundary.
    assert_eq!(
        check_stream(&store, &config, &mut state).await?,
        Some(PauseReason::ConsumerBacklog)
    );
    assert_eq!(state.stream_length, Some(451));
    let oldest = store.stream_groups(&stream, &config.groups).await?.unwrap();
    assert_eq!(
        oldest
            .iter()
            .find(|g| g.name == "archiver")
            .unwrap()
            .oldest_pending
            .unwrap()
            .to_string(),
        first
    );
    let retained: Vec<(String, Vec<(String, String)>)> = redis::cmd("XRANGE")
        .arg(&stream)
        .arg(&first)
        .arg(&first)
        .query_async(&mut con)
        .await?;
    assert_eq!(retained.len(), 1);
    // The old pending event, rather than recent acknowledged IDs, determines age.
    assert!(
        state.oldest_unconfirmed_age_secs.unwrap() > chrono::Utc::now().timestamp() as f64 - 2.0
    );
    // Work queued during the pause remains durable in PostgreSQL.
    let tag = format!("audit-{}", ulid::Ulid::generate());
    svc.create_user(
        Profile::new(format!("{tag}@example.com"), tag),
        CredentialType::Password,
        "test-only-hash",
        None,
        TrustedAuditContext::admin_control_plane(
            TrustedAdminActor::admin("01JZ000000000000000000000A"),
            TrustedAuditRequest::from_http(ulid::Ulid::generate().to_string(), None, None, None),
        ),
    )
    .await?;
    let before = svc.outbox_backlog().await?;
    assert!(before.pending_events > 0);
    assert_eq!(
        check_stream(&store, &config, &mut state).await?,
        Some(PauseReason::ConsumerBacklog)
    );
    for _ in 0..3 {
        assert_eq!(
            relay_checked_once(&svc, &store, &config, &mut state).await?,
            None
        );
    }
    assert_eq!(state.relay, "paused");
    assert_eq!(svc.outbox_backlog().await?, before);
    let _: u64 = redis::cmd("XACK")
        .arg(&stream)
        .arg("archiver")
        .arg(&first)
        .query_async(&mut con)
        .await?;
    assert_eq!(check_stream(&store, &config, &mut state).await?, None);
    assert!(state.stream_length.unwrap() < 450);
    assert_eq!(state.oldest_unconfirmed_age_secs, Some(0.0));
    assert!(
        relay_checked_once(&svc, &store, &config, &mut state)
            .await?
            .unwrap()
            > 0
    );
    assert_eq!(state.relay, "running");
    let mut unbounded = config.clone();
    unbounded.maxlen = None;
    assert_eq!(check_stream(&store, &unbounded, &mut state).await?, None);
    let _: u64 = redis::cmd("XGROUP")
        .arg("DESTROY")
        .arg(&stream)
        .arg("replica")
        .query_async(&mut con)
        .await?;
    assert_eq!(
        check_stream(&store, &config, &mut state).await?,
        Some(PauseReason::GroupMissing)
    );
    let _: u64 = redis::cmd("DEL").arg(&stream).query_async(&mut con).await?;
    Ok(())
}
