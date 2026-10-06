//! Correctness checks against an explicitly supplied disposable Redis instance.
use crate::api::gateway::authentication::TestAuthenticator;
use crate::etc::{
    auth::{
        identity::VerifiedIdentity,
        subject::{Subject, SubjectType},
    },
    gate::{Gate, prepare_config},
};
use axum::{body::Body, extract::ConnectInfo, response::Response};
use gate::cfg::Config;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use store::{AtomicStore, Store};

fn config(namespace: &str) -> Config {
    serde_json::from_value(json!({
        "schema":"stargate/v1",
        "ingress":{"limit":namespace, "timeout":"250ms"},
        "runtime":{"primary_concurrency":32, "request_timeout":"5s"},
        "limits":{
            "default":{"strategy":"gcra", "params":{"max_burst":1_000_000,"replenish_1_per":"1s"}},
            namespace:{"strategy":"gcra", "params":{"max_burst":2,"replenish_1_per":"1h"}},
            "rate":{"strategy":"gcra", "params":{"max_burst":2,"replenish_1_per":"1h"}},
            "quota":{"strategy":"quota_tracker", "params":{"limit":6,"period":"day"}}
        },
        "http":{
            "services":{"local":{"kind":"direct_response","status":200}},
            "policies":{
                "rate":{"kind":"rate_limit","limit":"rate"},
                "quota":{"kind":"quota","limit":"quota"}
            },
            "routers":{
                "rate":{"match":{"path":{"exact":"/rate"}},"service":"local","policies":["rate"]},
                "quota":{"match":{"path":{"exact":"/quota"}},"service":"local","policies":["quota"],"quota_cost":3},
                "open":{"priority":-100,"match":{"path":{"prefix":"/"}},"service":"local"}
            }
        }
    })).unwrap()
}

async fn send(
    gate: &Arc<Gate>,
    calls: &Arc<AtomicUsize>,
    path: &str,
    ip: &str,
    identity: VerifiedIdentity,
) -> Response {
    let mut request = Request::builder().uri(path).body(Body::empty()).unwrap();
    request.extensions_mut().insert(gate.clone());
    request.extensions_mut().insert(ConnectInfo(
        format!("{ip}:1000").parse::<SocketAddr>().unwrap(),
    ));
    let calls = calls.clone();
    request.extensions_mut().insert(TestAuthenticator {
        verify: Arc::new(move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            identity.clone()
        }),
    });
    crate::api::gateway::service(request).await.unwrap()
}

async fn code(response: Response) -> String {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<Value>(&body).unwrap()["code"]
        .as_str()
        .unwrap()
        .into()
}

fn identity(subject: &str, attrs: Value) -> VerifiedIdentity {
    VerifiedIdentity::test_api_key(Subject::new(
        subject.into(),
        SubjectType::ApiKey,
        Some(attrs),
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires STARGATE_BENCH_REDIS_URL pointing at disposable Redis"]
async fn cluster_gateway_redis_integration() {
    let url = std::env::var("STARGATE_BENCH_REDIS_URL").expect("explicit isolated Redis URL");
    let state = Arc::new(store::RedisStore::new(&url).await.unwrap());
    let other_state = Arc::new(store::RedisStore::new(&url).await.unwrap());
    let namespace = format!("verification-{}", ulid::Ulid::generate());
    let raw = config(&namespace);
    let gate = Arc::new(
        Gate::new(
            state.clone(),
            prepare_config(raw.clone()).unwrap(),
            gate::PolicySnapshot::default(),
        )
        .unwrap(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    for _ in 0..2 {
        assert_eq!(
            send(
                &gate,
                &calls,
                "/open",
                "203.0.113.1",
                VerifiedIdentity::anonymous()
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    let before = calls.load(Ordering::SeqCst);
    let denied = send(
        &gate,
        &calls,
        "/open",
        "203.0.113.1",
        VerifiedIdentity::anonymous(),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(denied.headers()["x-ratelimit-scope"], "ingress");
    assert!(
        denied.headers()["retry-after"]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    assert_eq!(code(denied).await, "gateway.ingress_rate_limit_exceeded");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        before,
        "denied ingress entered authentication"
    );
    gate.activate(prepare_config(raw.clone()).unwrap()).unwrap();
    assert_eq!(
        send(
            &gate,
            &calls,
            "/open",
            "203.0.113.1",
            VerifiedIdentity::anonymous()
        )
        .await
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );

    // Subject buckets stay shared across source IPs, independently of ingress.
    for (path, expected_code) in [
        ("/rate", "gateway.rate_limit_exceeded"),
        ("/quota", "gateway.quota_exceeded"),
    ] {
        let subject = format!("{namespace}{path}");
        for index in 0..3 {
            let response = send(
                &gate,
                &calls,
                path,
                &format!("203.0.113.{}", 20 + index),
                identity(&subject, json!({})),
            )
            .await;
            if index < 2 {
                assert_eq!(response.status(), StatusCode::OK);
                if path == "/quota" {
                    assert_eq!(
                        response.headers()["x-quota-remaining"],
                        (3 - index * 3).to_string()
                    );
                }
            } else {
                assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
                assert!(
                    response.headers()["retry-after"]
                        .to_str()
                        .unwrap()
                        .parse::<u64>()
                        .unwrap()
                        > 0
                );
                assert_eq!(code(response).await, expected_code);
            }
        }
    }
    let missing = send(
        &gate,
        &calls,
        "/open",
        "203.0.113.30",
        identity(&namespace, json!({"rate_limit":"missing"})),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(code(missing).await, "gateway.limit_configuration_invalid");

    // Process overload must not spend a Redis ingress allowance.
    let occupied: Vec<_> = (0..32)
        .map(|_| gate.resources.admit_primary().unwrap())
        .collect();
    let before = calls.load(Ordering::SeqCst);
    assert_eq!(
        send(
            &gate,
            &calls,
            "/open",
            "203.0.113.40",
            VerifiedIdentity::anonymous()
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(calls.load(Ordering::SeqCst), before);
    drop(occupied);
    assert!(
        state
            .get_i64(&format!("{namespace}:ingress:ip:203.0.113.40"))
            .await
            .unwrap()
            .is_none()
    );

    // A real Redis type error fails closed; restoring the key restores service.
    let corrupt = format!("{namespace}:ingress:ip:203.0.113.50");
    let mut connection = state.get_connection();
    redis::cmd("LPUSH")
        .arg(&corrupt)
        .arg("invalid")
        .query_async::<u64>(&mut connection)
        .await
        .unwrap();
    let before = calls.load(Ordering::SeqCst);
    assert_eq!(
        code(
            send(
                &gate,
                &calls,
                "/open",
                "203.0.113.50",
                VerifiedIdentity::anonymous()
            )
            .await
        )
        .await,
        "gateway.ingress_unavailable"
    );
    assert_eq!(calls.load(Ordering::SeqCst), before);
    state.delete(&corrupt).await.unwrap();
    assert_eq!(
        send(
            &gate,
            &calls,
            "/open",
            "203.0.113.50",
            VerifiedIdentity::anonymous()
        )
        .await
        .status(),
        StatusCode::OK
    );

    let mut short = raw;
    short.ingress.timeout = "30ms".into();
    gate.activate(prepare_config(short).unwrap()).unwrap();
    redis::cmd("CLIENT")
        .arg("PAUSE")
        .arg(200)
        .arg("ALL")
        .query_async::<()>(&mut connection)
        .await
        .unwrap();
    let before = calls.load(Ordering::SeqCst);
    let timed_out = send(
        &gate,
        &calls,
        "/open",
        "203.0.113.60",
        VerifiedIdentity::anonymous(),
    )
    .await;
    assert_eq!(timed_out.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(code(timed_out).await, "gateway.ingress_unavailable");
    assert_eq!(calls.load(Ordering::SeqCst), before);
    assert_eq!(gate.resources.available().0, 32);
    tokio::time::timeout(Duration::from_secs(3), state.ping())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        send(
            &gate,
            &calls,
            "/open",
            "203.0.113.61",
            VerifiedIdentity::anonymous()
        )
        .await
        .status(),
        StatusCode::OK
    );

    // Independent Redis pools compete for the same atomic allowances.
    let clock = lim::CachedClock::new();
    for (strategy, params, cost, admitted) in [
        ("gcra", json!({"max_burst":8,"replenish_1_per":"1h"}), 1, 8),
        (
            "token_bucket",
            json!({"capacity":8_000_000,"refill_rate":1}),
            1_000_000,
            8,
        ),
        ("quota_tracker", json!({"limit":6,"period":"day"}), 3, 2),
    ] {
        let mut tasks = tokio::task::JoinSet::new();
        let key = format!("{namespace}:{strategy}");
        let barrier = Arc::new(tokio::sync::Barrier::new(32));
        for index in 0..32 {
            let limit: gate::cfg::Limit = serde_json::from_value(
                json!({"name":strategy,"strategy":strategy,"params":params}),
            )
            .unwrap();
            let backend = if index % 2 == 0 {
                state.clone()
            } else {
                other_state.clone()
            };
            let limit = limit.build(backend, clock.clone()).unwrap();
            let key = key.clone();
            let barrier = barrier.clone();
            tasks.spawn(async move {
                barrier.wait().await;
                limit.check(&key, cost).await.unwrap().allowed
            });
        }
        let mut allowed = 0;
        while let Some(result) = tasks.join_next().await {
            allowed += usize::from(result.unwrap());
        }
        assert_eq!(allowed, admitted, "atomic {strategy} allowance exceeded");
    }
    gate.resources.begin_shutdown();
    assert_eq!(
        code(
            send(
                &gate,
                &calls,
                "/open",
                "203.0.113.70",
                VerifiedIdentity::anonymous()
            )
            .await
        )
        .await,
        "gateway.cancelled"
    );
    assert_eq!(gate.resources.available().0, 32);
    if let Ok(path) = std::env::var("STARGATE_BENCH_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&json!({"passed":true,"backend":"redis","checks":["pre_auth_ingress","reload_allowances","subject_rate","weighted_quota","unknown_limit","process_admission","redis_error_recovery","redis_deadline_recovery","atomic_gcra","atomic_token_bucket","atomic_quota","shutdown"]})).unwrap()).unwrap();
    }
}
