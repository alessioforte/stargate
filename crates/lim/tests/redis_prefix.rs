#![cfg(feature = "redis")]

use lim::{
    CachedClock,
    strategies::{
        RateLimit,
        gcra::{Gcra, Quota},
        quota_tracker::{QuotaTracker, ShortWindow},
        token_bucket::{TokenBucket, TokenBucketConfig},
    },
};
use std::{sync::Arc, time::Duration};
use store::Store;

#[tokio::test]
#[ignore = "requires isolated REDIS_PREFIX_TEST_URL with the restricted Stargate ACL"]
async fn all_lua_limiters_use_the_store_prefix() -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(
        store::RedisStore::new(&std::env::var("REDIS_PREFIX_TEST_URL")?)
            .await?
            .with_key_prefix("tenant:"),
    );
    let clock = CachedClock::new();
    let gcra = Gcra::new(
        store.clone(),
        clock.clone(),
        Quota::per_duration(1, Duration::from_secs(3600)),
    );
    let token = TokenBucket::new(store.clone(), clock.clone(), TokenBucketConfig::new(1, 0));
    let quota = QuotaTracker::new(store.clone(), clock, 1, ShortWindow::Hour);
    let tag = format!("test:{}", ulid::Ulid::generate());
    for (name, limiter) in [
        ("gcra", &gcra as &dyn RateLimit),
        ("token", &token as &dyn RateLimit),
        ("quota", &quota as &dyn RateLimit),
    ] {
        let key = store::keys::scoped(&tag, name);
        assert!(limiter.check(&key, 1).await?.allowed);
        assert!(!limiter.check(&key, 1).await?.allowed);
    }

    // A denied request over a cost larger than the limit must not create the
    // bucket: a key without a TTL would live forever.
    let denied = store::keys::scoped(&tag, "quota-denied");
    assert!(!quota.check(&denied, 2).await?.allowed);
    assert!(!store.exists(&denied).await?);
    Ok(())
}
