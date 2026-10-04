use super::tests::{available, replay_request, replay_state};
use crate::api::gateway::{
    execution::execute_plan_from_replay, mirrors::execute_mirror, types::ExecutionPlan,
};
use axum::{body::Body, response::Response};
use ctx::DispatchKind;
use futures_util::stream;
use gate::cfg::RuntimeConfig;
use hyper::{body::Bytes, service::service_fn};
use hyper_util::rt::TokioIo;
use std::{
    convert::Infallible,
    io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};

const SERVICE: &str = "disposal-test";

#[derive(Clone, Copy)]
enum BodyMode {
    Small,
    Endless,
    Stalled,
    Error,
    Large(u64),
}

struct BodyGuard(Arc<AtomicUsize>);

impl Drop for BodyGuard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct MockUpstream {
    runtime: Arc<crate::etc::gate::RuntimeSnapshot>,
    url: String,
    connections: Arc<AtomicUsize>,
    dropped_bodies: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl Drop for MockUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn mock_upstream(mode: BodyMode) -> MockUpstream {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let connections = Arc::new(AtomicUsize::new(0));
    let dropped_bodies = Arc::new(AtomicUsize::new(0));
    let accepted = connections.clone();
    let dropped = dropped_bodies.clone();
    let task = tokio::spawn(async move {
        let mut sessions = JoinSet::new();
        loop {
            tokio::select! {
                socket = listener.accept() => {
                    let (socket, _) = socket.unwrap();
                    accepted.fetch_add(1, Ordering::SeqCst);
                    let dropped = dropped.clone();
                    sessions.spawn(async move {
                        let service = service_fn(move |_| {
                            let body = generated_body(mode, BodyGuard(dropped.clone()));
                            let mut builder = Response::builder().status(503);
                            if let BodyMode::Large(bytes) = mode {
                                builder = builder.header("content-length", bytes);
                            }
                            async move { Ok::<_, Infallible>(builder.body(body).unwrap()) }
                        });
                        let _ = hyper::server::conn::http1::Builder::new()
                            .serve_connection(TokioIo::new(socket), service).await;
                    });
                }
                _ = sessions.join_next(), if !sessions.is_empty() => {}
            }
        }
    });
    let config = RuntimeConfig::from_raw(
        serde_json::from_value(serde_json::json!({
            "schema": gate::cfg::SCHEMA,
            "ingress": {"limit": "ingress", "timeout": "250ms"},
            "limits": {"default": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "1s"}}, "ingress": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "100ms"}}},
            "http": {
                "upstreams": {SERVICE: {"targets": [{"url": url}], "load_balancer": {"strategy": "round_robin", "circuit_breaker": {"fail_threshold": 1, "cooldown": "3600s"}}}},
                "services": {SERVICE: {"kind": "load_balancer", "upstream": SERVICE}},
            },
        }))
        .unwrap(),
    )
    .unwrap();
    MockUpstream {
        runtime: crate::etc::gate::test_support::runtime(config),
        url,
        connections,
        dropped_bodies,
        task,
    }
}

fn generated_body(mode: BodyMode, guard: BodyGuard) -> Body {
    let body = stream::unfold((0_u64, guard), move |(sent, guard)| async move {
        if sent != 0 {
            match mode {
                BodyMode::Small => return None,
                BodyMode::Stalled => std::future::pending::<()>().await,
                BodyMode::Error => {
                    // Let response headers and the first frame reach the client.
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    return Some((Err(io::Error::other("body stream failed")), (sent, guard)));
                }
                _ => {}
            }
        }
        let chunk = match mode {
            BodyMode::Small => Bytes::from_static(b"small"),
            BodyMode::Large(size) => {
                if sent >= size {
                    return None;
                }
                Bytes::from_static(&[0; 16 * 1024]).slice(..(size - sent).min(16 * 1024) as usize)
            }
            _ => Bytes::from_static(&[0; 16 * 1024]),
        };
        let next = sent + chunk.len() as u64;
        Some((Ok::<_, io::Error>(chunk), (next, guard)))
    });
    Body::from_stream(body)
}

fn plan(failover: bool) -> ExecutionPlan {
    let upstream = ExecutionPlan::Upstream {
        service_name: SERVICE.to_string(),
        internal_context: None,
    };
    if !failover {
        return upstream;
    }
    ExecutionPlan::Failover {
        services: vec![
            upstream,
            ExecutionPlan::DirectResponse {
                status: 204,
                headers: Vec::new(),
                body: None,
            },
        ],
        on_status: vec![503],
    }
}

async fn wait_for_dropped_body(upstream: &MockUpstream, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while upstream.dropped_bodies.load(Ordering::SeqCst) < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("upstream body was not released");
}

#[tokio::test]
async fn failover_advances_after_small_endless_stalled_and_broken_responses() {
    for mode in [
        BodyMode::Small,
        BodyMode::Endless,
        BodyMode::Stalled,
        BodyMode::Error,
    ] {
        let upstream = mock_upstream(mode).await;
        let balancers = &upstream.runtime.core.balancers;
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            execute_plan_from_replay(
                &upstream.runtime,
                &plan(true),
                &replay_request(),
                &replay_state(),
                DispatchKind::Primary,
            ),
        )
        .await
        .expect("discarded body prevented failover")
        .unwrap();
        assert_eq!(response.status(), 204);
        assert!(
            !available(&balancers, SERVICE),
            "primary 503 should still affect upstream health"
        );
        wait_for_dropped_body(&upstream, 1).await;
    }
}

#[tokio::test]
async fn mirror_completion_distinguishes_disposal_outcomes_without_changing_health() {
    for (mode, expected) in [
        (BodyMode::Small, "success"),
        (BodyMode::Endless, "abandoned"),
        (BodyMode::Stalled, "timeout"),
        (BodyMode::Error, "body_error"),
    ] {
        let upstream = mock_upstream(mode).await;
        let balancers = &upstream.runtime.core.balancers;
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            execute_mirror(
                &upstream.runtime,
                &plan(false),
                &replay_request(),
                &replay_state(),
            ),
        )
        .await
        .expect("mirror disposal did not terminate");
        assert_eq!(outcome, expected);
        assert!(
            available(&balancers, SERVICE),
            "shadow 503/body errors must not trip the breaker"
        );
        wait_for_dropped_body(&upstream, 1).await;
    }
}

#[tokio::test]
async fn small_discarded_responses_allow_http_connection_reuse() {
    let upstream = mock_upstream(BodyMode::Small).await;
    for _ in 0..2 {
        assert_eq!(
            execute_mirror(
                &upstream.runtime,
                &plan(false),
                &replay_request(),
                &replay_state()
            )
            .await,
            "success"
        );
    }
    assert_eq!(upstream.connections.load(Ordering::SeqCst), 1);
    wait_for_dropped_body(&upstream, 2).await;
}

#[tokio::test]
#[ignore = "large response memory measurement; requires localhost socket access"]
async fn large_response_disposal_workload() {
    let bytes = std::env::var("STARGATE_DISPOSAL_WORKLOAD_BYTES")
        .ok()
        .map(|value| value.parse::<u64>().unwrap())
        .unwrap_or(256 * 1024 * 1024);
    assert!(bytes > 1024 * 1024);
    let upstream = mock_upstream(BodyMode::Large(bytes)).await;
    for iteration in 0..32 {
        // This workload measures disposal, so re-enable its target between
        // primary failures and shadow dispatches rather than testing cooldown.
        upstream.runtime.core.balancers[SERVICE].mark_alive(&upstream.url);
        let response = execute_plan_from_replay(
            &upstream.runtime,
            &plan(true),
            &replay_request(),
            &replay_state(),
            DispatchKind::Primary,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 204);
        upstream.runtime.core.balancers[SERVICE].mark_alive(&upstream.url);
        assert_eq!(
            execute_mirror(
                &upstream.runtime,
                &plan(false),
                &replay_request(),
                &replay_state()
            )
            .await,
            "abandoned"
        );
        wait_for_dropped_body(&upstream, (iteration + 1) * 2).await;
    }
    println!("Discarded 32 failover and 32 mirror responses, each declaring {bytes} bytes");
}
