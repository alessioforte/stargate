#![cfg(feature = "redis")]

//! Lua limiters against a Redis Cluster node.
//!
//! Every script declares all the keys it touches, and those keys carry the
//! entity hash tag, so the node accepts them in one slot. A derived key inside
//! a script would fail here with `non local key`; two cross-slot keys with
//! `CROSSSLOT`.

use lim::{
    CachedClock,
    strategies::{
        RateLimit,
        gcra::{Gcra, Quota},
        quota_tracker::{QuotaTracker, TimeWindow},
        token_bucket::{TokenBucket, TokenBucketConfig},
    },
};
use std::{sync::Arc, time::Duration};

#[tokio::test]
#[ignore = "requires a Redis Cluster node that owns REDIS_CLUSTER_TAG"]
async fn all_lua_limiters_keep_their_keys_in_the_entity_slot()
-> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("REDIS_CLUSTER_URL")?;
    let tag = std::env::var("REDIS_CLUSTER_TAG")?;
    let store = Arc::new(
        store::RedisStore::new(&url)
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
    let quota = QuotaTracker::new(store, clock, 1, TimeWindow::Day);
    for (name, limiter) in [
        ("gcra", &gcra as &dyn RateLimit),
        ("token", &token as &dyn RateLimit),
        ("quota", &quota as &dyn RateLimit),
    ] {
        let key = store::keys::scoped(&tag, name);
        assert!(limiter.check(&key, 1).await?.allowed, "{name}");
        assert!(!limiter.check(&key, 1).await?.allowed, "{name}");
    }
    Ok(())
}
