//! Opt-in release workloads. See docs/gateway-performance.md for scope and commands.
mod combined;
mod fixtures;
mod reporting;
mod routes;

use crate::etc::{
    auth::jwt as session_jwt, gate::prepare_config, internal_context, store::use_store,
};
use fixtures::{Fixture, Scenario};
use reporting::{Metrics, Sampler, latencies};
use serde_json::json;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use store::Store;
use tokio::task::JoinSet;

fn seconds() -> u64 {
    std::env::var("STARGATE_BENCH_SECONDS").map_or(3, |v| v.parse().unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "release benchmark with localhost sockets and explicit fixture environment"]
async fn gateway_release_workloads() {
    if cfg!(debug_assertions) {
        panic!("run this workload with --release");
    }
    assert_eq!(std::env::var("JWT_ALGORITHM").unwrap(), "HS256");
    session_jwt::init();
    internal_context::init().unwrap();
    let metrics = Metrics::new();
    let session = ulid::Ulid::generate().to_string();
    let subject = crate::etc::auth::subject::Subject::new(
        session.clone(),
        crate::etc::auth::subject::SubjectType::User,
        None,
    );
    use_store()
        .set(&session, &subject, Some(600))
        .await
        .unwrap();
    let token = session_jwt::jwt_config()
        .generate_session_access_token(jwt::Claims {
            sub: session.clone(),
            sid: Some(session.clone()),
            auth_time: Some(chrono::Utc::now().timestamp() as usize),
            ..jwt::Claims::default()
        })
        .unwrap();
    let mut results = Vec::new();
    for scenario in Scenario::ALL {
        if std::env::var("STARGATE_BENCH_SCENARIOS")
            .is_ok_and(|names| !names.split(',').any(|name| name == scenario.name()))
        {
            continue;
        }
        let fixture = Fixture::new(scenario).await;
        // Warm up connections and the verifier before starting measurements.
        fixture
            .request(scenario.path(), scenario.auth().then_some(&token), 0)
            .await;
        let cancellation = tokio_util::sync::CancellationToken::new();
        let reload = (scenario == Scenario::Reload).then(|| {
            let gate = fixture.gate.clone();
            let config = fixture.config.clone();
            let cancellation = cancellation.clone();
            tokio::spawn(async move {
                let mut count = 0;
                while !cancellation.is_cancelled() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    gate.activate(prepare_config(config.clone()).unwrap())
                        .unwrap();
                    count += 1;
                }
                count
            })
        });
        let background = fixture.background(scenario).await;
        let counters = metrics.counters();
        let sampler = Sampler::start(fixture.gate.resources.clone());
        let started = Instant::now();
        let deadline = started + Duration::from_secs(seconds());
        let mut tasks = JoinSet::new();
        for worker in 0..scenario.concurrency() {
            let fixture = fixture.clone();
            let token = token.clone();
            tasks.spawn(async move {
                let mut timings = Vec::new();
                let mut headers = Vec::new();
                let mut statuses = BTreeMap::new();
                let mut short = Vec::new();
                while Instant::now() < deadline {
                    let path = if scenario == Scenario::MixedSigning && worker % 2 == 0 {
                        "/short"
                    } else {
                        scenario.path()
                    };
                    let start = Instant::now();
                    let (status, header_time) = fixture
                        .request(
                            path,
                            scenario.auth().then_some(token.as_str()),
                            scenario.body_bytes(),
                        )
                        .await;
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                    timings.push(elapsed);
                    headers.push(header_time);
                    *statuses.entry(status).or_insert(0_u64) += 1;
                    if path == "/short" {
                        short.push(elapsed);
                    }
                }
                (timings, headers, statuses, short)
            });
        }
        let mut timings = Vec::new();
        let mut headers = Vec::new();
        let mut statuses = BTreeMap::new();
        let mut short = Vec::new();
        while let Some(result) = tasks.join_next().await {
            let (t, h, s, u) = result.unwrap();
            timings.extend(t);
            headers.extend(h);
            short.extend(u);
            for (status, count) in s {
                *statuses.entry(status).or_insert(0_u64) += count;
            }
        }
        let elapsed = started.elapsed().as_secs_f64();
        let samples = sampler.finish();
        cancellation.cancel();
        let reloads = if let Some(task) = reload {
            task.await.unwrap()
        } else {
            0
        };
        for task in background {
            assert!(
                !task.is_finished(),
                "stream or WebSocket stopped during the workload"
            );
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        }
        // Let all admitted mirrors expire before examining counters and retained state.
        tokio::time::sleep(Duration::from_millis(350)).await;
        let after = metrics.counters();
        let counters: BTreeMap<_, _> = after
            .into_iter()
            .filter_map(|(key, value)| {
                let delta = value.saturating_sub(*counters.get(&key).unwrap_or(&0));
                (delta > 0).then_some((key, delta))
            })
            .collect();
        let resources = &fixture.gate.resources;
        let available = resources.available();
        assert_eq!(
            available,
            (
                resources.budgets.primary_concurrency,
                resources.budgets.mirror_concurrency,
                resources.budgets.replay_memory_bytes
            )
        );
        assert_eq!(resources.tracked_tasks(), 0);
        assert!(
            samples["mirror_peak"].as_u64().unwrap() <= resources.budgets.mirror_concurrency as u64
        );
        assert!(
            samples["replay_peak_bytes"].as_u64().unwrap()
                <= resources.budgets.replay_memory_bytes as u64
        );
        assert!(!statuses.contains_key(&429), "benchmark was throttled");
        let successes = *statuses.get(&200).unwrap_or(&0);
        assert!(successes > 0, "no successful requests: {statuses:?}");
        if !matches!(scenario, Scenario::Admission | Scenario::ReplaySaturation) {
            assert_eq!(statuses.len(), 1, "unexpected response: {statuses:?}");
        }
        if scenario == Scenario::SlowMirror {
            assert!(counters.keys().any(|key| key.contains("skipped_capacity")));
        }
        let connections = fixture.connection_counts();
        if matches!(scenario, Scenario::Plain | Scenario::Authenticated) {
            assert!(
                connections["upstream"].as_u64().unwrap() <= scenario.concurrency() as u64 * 3,
                "steady traffic rebuilt client pools"
            );
        }
        let result = json!({"connections":connections, "runtime":fixture.config.runtime, "scenario":scenario.name(), "seconds":elapsed, "concurrency":scenario.concurrency(), "body_bytes":scenario.body_bytes(), "requests":timings.len(), "requests_per_second":timings.len() as f64 / elapsed, "successful_requests_per_second":successes as f64 / elapsed, "statuses":statuses, "body_latency_ms":latencies(timings), "header_latency_ms":latencies(headers), "short_latency_ms":latencies(short), "samples":samples, "counters":counters, "reloads":reloads});
        println!("GATEWAY_BENCH {result}");
        results.push(result);
        fixture.stop().await;
    }
    use_store().delete(&session).await.unwrap();
    assert!(!results.is_empty(), "no matching benchmark scenarios");
    let route_results = if std::env::var("STARGATE_BENCH_SCENARIOS").is_ok() {
        Vec::new()
    } else {
        routes::measure()
    };
    if let Ok(path) = std::env::var("STARGATE_BENCH_OUTPUT") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(
                &json!({"workers":2, "scenarios":results, "routing":route_results}),
            )
            .unwrap(),
        )
        .unwrap();
    }
}
