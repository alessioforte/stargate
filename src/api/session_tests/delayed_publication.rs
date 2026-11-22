use super::*;
use crate::iam::sess::{
    subjects::{self, PublishedSubject},
    test_support::{Pause, Stage},
};
use crate::srv::health::Lifecycle;
use redis::AsyncCommands;
use std::time::Duration;
use store::{RedisPoolConfig, RedisStore};

pub(super) async fn run(router: &Router, admin: (&str, &str)) {
    let db = db::service();
    assert_eq!(db.pool_stats().max, 1);
    let mut user = create_user().await;
    let (sid, native, _, oauth) = start_session(&user, None).await;
    let reduced = json!({"grants": ["read"]});
    concurrency::set_grants(router, admin, &user, &["read"]).await;
    user.attrs = json!({"grants": ["read", "write"]});
    db::update_user(user.clone(), context()).await.unwrap();

    let live = use_store();
    let key = store::keys::session::subject(&sid);
    let physical = live.prefixed_key(&key);
    let mut connection = live.get_connection();
    let expires: i64 = connection.pexpire_time(physical.as_ref()).await.unwrap();
    assert!(expires > 0);
    let previous = live.get::<PublishedSubject>(&key).await.unwrap().unwrap();

    // BLPOP holds only this connection on the server. The multiplexed client
    // times out while Redis still has the publication queued behind BLPOP.
    let delayed = RedisStore::with_config(
        &std::env::var("REDIS_PREFIX_TEST_URL").unwrap(),
        RedisPoolConfig::new(1).with_response_timeout(Some(Duration::from_secs(2))),
    )
    .await
    .unwrap()
    .with_key_prefix(std::env::var("REDIS_KEY_PREFIX").unwrap())
    .with_compression(live.compression().clone());
    let gate = format!("{physical}:test-wait");
    let pause = Pause::new(Stage::SubjectWrite);
    let publication = pause.scope(subjects::publish(&delayed, &db, &user.id, &sid, None));
    let hold = async {
        pause.wait().await;
        let error = delayed
            .get_connection()
            .blpop::<_, Option<(String, String)>>(&gate, 30.0)
            .await
            .unwrap_err();
        assert!(error.is_timeout(), "{error}");
        pause.resume();
        // Let the publisher enqueue its CAS before probing the single SQL connection.
        tokio::task::yield_now().await;
        let lifecycle = Lifecycle::default();
        lifecycle.mark_ready();
        let req = Request::get("/readyz").body(Body::empty()).unwrap();
        send(&crate::api::health::router(lifecycle), req, StatusCode::OK).await;
    };
    let (result, ()) = tokio::join!(publication, hold);
    let error = result.unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains("timed out"),
        "{error}"
    );

    concurrency::set_grants(router, admin, &user, &["read"]).await;
    let tokens = [native.as_str(), oauth.as_str()];
    assert_subject(tokens, &reduced, None).await;
    let _: usize = connection.rpush(&gate, "resume").await.unwrap();
    delayed.ping().await.unwrap(); // Ordered after the late write on the same socket.
    assert_subject(tokens, &reduced, None).await;
    assert_eq!(
        connection
            .pexpire_time::<_, i64>(physical.as_ref())
            .await
            .unwrap(),
        expires
    );
    assert_eq!(connection.llen::<_, usize>(&gate).await.unwrap(), 0);

    let current = live.get::<PublishedSubject>(&key).await.unwrap().unwrap();
    assert!(current.revision > previous.revision);
    revise_session(&sid, |record| record.client_ids.clear()).await;
    registry::revoke_session(&sid).await.unwrap();
    assert!(
        !delayed
            .replace_if_equal(&key, &current, &previous)
            .await
            .unwrap()
    );
    assert!(!live.exists(&key).await.unwrap());
}
