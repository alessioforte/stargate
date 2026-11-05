#![cfg(feature = "redis")]

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
#[ignore = "requires isolated SG02_REDIS_URL with the restricted Stargate ACL"]
async fn all_lua_limiters_use_the_store_prefix() -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(
        store::RedisStore::new(&std::env::var("SG02_REDIS_URL")?)
            .await?
            .with_key_prefix("stargate:"),
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
        assert!(
            limiter
                .check(&format!("lim:prefix:{name}"), 1)
                .await?
                .allowed
        );
        assert!(
            !limiter
                .check(&format!("lim:prefix:{name}"), 1)
                .await?
                .allowed
        );
    }
    Ok(())
}
