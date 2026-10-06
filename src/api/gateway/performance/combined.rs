use crate::api::gateway::performance::{
    fixtures::{Fixture, Scenario},
    reporting::{Metrics, Sampler, latencies},
    seconds,
};
use crate::etc::{
    gate::{
        reload_gateway_config_from_path,
        test_support::{CLIENT_KEY, TlsFiles},
    },
    internal_context,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::task::JoinSet;

fn histogram<'a>(values: &'a BTreeMap<String, Value>, name: &str, labels: &[&str]) -> &'a Value {
    values
        .iter()
        .find(|(key, _)| key.starts_with(name) && labels.iter().all(|label| key.contains(label)))
        .unwrap()
        .1
}

async fn reload_during_traffic(fixture: Fixture) -> u64 {
    let files = TlsFiles::new();
    let path = files.path("config.yaml");
    for index in 0..10 {
        tokio::time::sleep(Duration::from_millis(40)).await;
        if index == 3 {
            let pinned = fixture.gate.snapshot();
            let mut secure = fixture.config.clone();
            secure.mtls = Some(files.mtls());
            secure.to_file(&path);
            std::fs::remove_file(files.path("client-key.pem")).unwrap();
            assert!(reload_gateway_config_from_path(&fixture.gate, &path).is_err());
            assert!(Arc::ptr_eq(&pinned, &fixture.gate.snapshot()));
            files.write("client-key.pem", b"invalid PEM key");
            assert!(reload_gateway_config_from_path(&fixture.gate, &path).is_err());
            assert!(Arc::ptr_eq(&pinned, &fixture.gate.snapshot()));
            files.write("client-key.pem", CLIENT_KEY);
            assert_eq!(
                reload_gateway_config_from_path(&fixture.gate, &path).unwrap(),
                pinned.version + 1
            );
        }
        fixture.config.to_file(&path);
        reload_gateway_config_from_path(&fixture.gate, &path).unwrap();
    }
    fixture.gate.snapshot().version
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "isolated release verification with localhost sockets and explicit fixture environment"]
async fn gateway_combined_verification() {
    if cfg!(debug_assertions) {
        panic!("run this verification with --release");
    }
    internal_context::init().unwrap();
    let metrics = Metrics::new();
    let fixture = Fixture::new(Scenario::SlowMirror).await;
    let resources = fixture.gate.resources.clone();

    // One paced body proves headers and completion have distinct instruments;
    // nested body guards must count the downstream response exactly once.
    let start = Instant::now();
    let (status, header_ms) = fixture.request("/stream", None, 0).await;
    let completion_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(status, 200);
    assert!(completion_ms > header_ms + 1.0);
    let initial = metrics.histograms();
    let headers = histogram(&initial, "stargate.gateway.duration{", &[]);
    let body = histogram(
        &initial,
        "stargate.gateway.transfer.duration{",
        &["phase=response_body", "stargate.outcome=complete"],
    );
    assert_eq!(headers["count"], 1);
    assert_eq!(body["count"], 1);
    assert!(body["sum"].as_f64().unwrap() > headers["sum"].as_f64().unwrap());

    let mut background = fixture.background(Scenario::LiveStreams).await;
    let first_snapshot = Arc::downgrade(&fixture.gate.snapshot());
    let sampler = Sampler::start(resources.clone());
    let deadline = Instant::now() + Duration::from_secs(seconds());
    let mut clients = JoinSet::new();
    for index in 0..16 {
        let fixture = fixture.clone();
        clients.spawn(async move {
            let path = if index < 4 {
                "/status"
            } else if index < 12 {
                "/slow-mirror"
            } else {
                "/short"
            };
            let bytes = if path == "/slow-mirror" { 64 * 1024 } else { 0 };
            let mut count = 0;
            let mut short = Vec::new();
            while Instant::now() < deadline {
                let (status, headers) = fixture.request(path, None, bytes).await;
                assert_eq!(status, 200, "combined traffic failed at {path}");
                count += 1;
                if path == "/short" {
                    short.push(headers);
                }
            }
            (path, count, short)
        });
    }
    let version = reload_during_traffic(fixture.clone()).await;
    assert_eq!(version, 11);
    assert!(
        first_snapshot.upgrade().is_some(),
        "live sessions lost their pinned snapshot"
    );
    assert!(background.iter().all(|task| !task.is_finished()));
    // Disconnect one stream and one WebSocket while the other two continue
    // through reloads and pressure. All four have exchanged real data.
    for task in background.drain(..2) {
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
    let mut requests = BTreeMap::new();
    let mut short = Vec::new();
    while let Some(result) = clients.join_next().await {
        let (path, count, values) = result.unwrap();
        *requests.entry(path).or_insert(0_u64) += count;
        short.extend(values);
    }
    assert!(background.iter().all(|task| !task.is_finished()));
    assert!(requests.values().all(|count| *count > 0));

    let occupied: Vec<_> = (0..resources.available().0)
        .map(|_| resources.admit_primary().unwrap())
        .collect();
    assert_eq!(fixture.request("/short", None, 0).await.0, 503);
    drop(occupied);
    tokio::time::timeout(Duration::from_secs(2), async {
        while resources.available().2 != resources.budgets.replay_memory_bytes {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let reserved = resources
        .reserve_replay(resources.budgets.replay_memory_bytes)
        .unwrap();
    assert_eq!(fixture.request("/status", None, 0).await.0, 503);
    drop(reserved);

    // Leave detached mirrors and live connections active when shutdown starts.
    for _ in 0..4 {
        assert_eq!(
            fixture.request("/slow-mirror", None, 64 * 1024).await.0,
            200
        );
    }
    assert_eq!(resources.available().1, 0);
    resources.begin_shutdown();
    tokio::time::timeout(Duration::from_secs(2), resources.wait())
        .await
        .unwrap();
    for task in background {
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while first_snapshot.upgrade().is_some()
            || resources.available()
                != (
                    resources.budgets.primary_concurrency,
                    resources.budgets.mirror_concurrency,
                    resources.budgets.replay_memory_bytes,
                )
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(resources.tracked_tasks(), 0);
    let peaks = sampler.finish();
    assert!(peaks["primary_peak"].as_u64().unwrap() <= 32);
    assert!(peaks["mirror_peak"].as_u64().unwrap() <= 4);
    assert!(peaks["replay_peak_bytes"].as_u64().unwrap() <= 1024 * 1024);
    assert!(peaks["tracked_tasks_peak"].as_u64().unwrap() <= 6);

    let counters = metrics.counters();
    for (name, label) in [
        ("stargate.gateway.failovers{", "reason=status"),
        ("stargate.gateway.mirrors{", "skipped_capacity"),
        ("stargate.gateway.timeouts{", "phase=response_headers"),
        ("stargate.gateway.timeouts{", "phase=disposal"),
        ("stargate.gateway.rejections{", "kind=primary"),
        ("stargate.gateway.rejections{", "kind=replay_memory"),
    ] {
        assert!(
            counters
                .iter()
                .any(|(key, value)| key.starts_with(name) && key.contains(label) && *value > 0),
            "missing {name}{label}: {counters:?}"
        );
    }
    for (outcome, count) in [("success", 11), ("transport_error", 2)] {
        assert_eq!(
            counters
                .iter()
                .find(|(key, _)| key.starts_with("stargate.config.reloads{")
                    && key.contains(&format!("stargate.outcome={outcome}")))
                .unwrap()
                .1,
            &count
        );
    }
    let active = metrics.active();
    assert!(!active.is_empty());
    assert!(
        active.values().all(|value| *value == 0),
        "leaked metric resources: {active:?}"
    );
    let histograms = metrics.histograms();
    for labels in [
        ["phase=response_body", "stargate.outcome=dropped"],
        ["phase=response_body", "stargate.outcome=cancelled"],
        ["phase=websocket", "stargate.outcome=cancelled"],
    ] {
        assert!(
            histogram(&histograms, "stargate.gateway.transfer.duration{", &labels)["count"]
                .as_u64()
                .unwrap()
                > 0
        );
    }
    let result = json!({
        "passed":true, "seconds":seconds(), "requests":requests,
        "short_header_ms":latencies(short), "resources":peaks,
        "final_available":resources.available(), "final_tracked_tasks":resources.tracked_tasks(),
        "reload_version":version, "tls_reload_rejections":2,
        "pinned_snapshot_drained":first_snapshot.upgrade().is_none(),
        "paced_body":{"header_ms":header_ms,"completion_ms":completion_ms},
        "counters":counters,"histograms":histograms,"active":active,
        "connections":fixture.connection_counts(),
    });
    std::fs::write(
        std::env::var("STARGATE_BENCH_OUTPUT").unwrap(),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("COMBINED_GATEWAY_VERIFICATION {result}");
    fixture.stop().await;
}
