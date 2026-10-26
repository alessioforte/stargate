//! Isolated backend round trips, deliberately separate from gateway CPU workloads.
use db::DbStore;
use serde_json::json;
use std::{sync::Arc, time::Instant};
use store::Store;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "release benchmark requiring explicit isolated Redis/PostgreSQL URLs"]
async fn cluster_backend_round_trips() {
    if cfg!(debug_assertions) {
        panic!("run with --release");
    }
    let redis_url = std::env::var("STARGATE_BENCH_REDIS_URL").unwrap();
    let database_url = std::env::var("STARGATE_BENCH_DATABASE_URL").unwrap();
    let state = Arc::new(store::RedisStore::new(&redis_url).await.unwrap());
    let database = db::service::init(&database_url).await.unwrap();
    let key = format!("benchmark:{}", ulid::Ulid::generate());
    let subject = crate::etc::auth::subject::Subject::new(
        key.clone(),
        crate::etc::auth::subject::SubjectType::User,
        Some(json!({"payload":"x".repeat(1024)})),
    );
    state.set(&key, &subject, Some(60)).await.unwrap();
    let clock = lim::CachedClock::new();
    let rate: gate::cfg::Limit = serde_json::from_value(json!({"name":"benchmark-rate", "strategy":"gcra", "params":{"max_burst":1_000_000, "replenish_1_per":"1us"}})).unwrap();
    let rate = rate.build(state.clone(), clock.clone()).unwrap();
    let quota: gate::cfg::Limit = serde_json::from_value(json!({"name":"benchmark-quota", "strategy":"quota_tracker", "params":{"limit":1_000_000, "period":"day"}})).unwrap();
    let quota = quota.build(state.clone(), clock).unwrap();
    let mut results = Vec::new();
    for operation in [
        "redis_session_get_1k",
        "redis_revocation_exists",
        "redis_gcra",
        "redis_quota",
        "postgres_select1",
        "postgres_org_lookup_miss",
    ] {
        let mut samples = Vec::new();
        for index in 0..1010 {
            let start = Instant::now();
            match operation {
                "redis_session_get_1k" => assert!(
                    state
                        .get::<crate::etc::auth::subject::Subject>(&key)
                        .await
                        .unwrap()
                        .is_some()
                ),
                "redis_revocation_exists" => {
                    assert!(!state.exists(&format!("{key}:revoked")).await.unwrap())
                }
                "redis_gcra" => {
                    assert!(rate.check(&format!("{key}:rate"), 1).await.unwrap().allowed)
                }
                "redis_quota" => assert!(
                    quota
                        .check(&format!("{key}:quota"), 1)
                        .await
                        .unwrap()
                        .allowed
                ),
                "postgres_select1" => database.ping().await.unwrap(),
                "postgres_org_lookup_miss" => assert!(
                    database
                        .get_organization_by_id(&key)
                        .await
                        .unwrap()
                        .is_none()
                ),
                _ => unreachable!(),
            }
            if index >= 10 {
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        samples.sort_by(f64::total_cmp);
        let at = |percent: usize| samples[(samples.len() * percent).div_ceil(100) - 1];
        let result = json!({"operation":operation,"samples":samples.len(),"round_trip_ms":{"p50":at(50),"p95":at(95),"p99":at(99),"max":samples.last().unwrap()}});
        println!("BACKEND_BENCH {result}");
        results.push(result);
    }
    state.delete(&key).await.unwrap();
    if let Ok(path) = std::env::var("STARGATE_BENCH_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
    }
}
