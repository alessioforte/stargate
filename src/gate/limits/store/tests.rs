use super::*;
use crate::gate::test_support;
use gate::cfg::{Config, RuntimeConfig};
use std::sync::{Arc, atomic::AtomicU8};

struct TestLimit {
    kind: LimitKind,
    state: Arc<AtomicU8>,
}

#[async_trait::async_trait]
impl lim::RateLimit for TestLimit {
    fn kind(&self) -> LimitKind {
        self.kind
    }

    async fn check(&self, _key: &str, _cost: u64) -> lim::Result<RateLimitDecision> {
        match self.state.load(Ordering::Relaxed) {
            0 => Ok(RateLimitDecision::allowed(100, 99, None)),
            1 => Err(RateLimitError::BackendError("unavailable".into())),
            2 => std::future::pending().await,
            3 => Err(RateLimitError::InvalidConfig(
                "invalid configuration".into(),
            )),
            4 => Err(RateLimitError::ClockError("clock".into())),
            _ => Ok(RateLimitDecision::denied(100, 0, None, None)),
        }
    }
}

fn limiter(state: &Arc<AtomicU8>) -> lim::Limiter {
    let mut limiter = lim::Limiter::new();
    for (name, kind) in [
        ("ingress", LimitKind::Rate),
        ("default", LimitKind::Rate),
        ("daily", LimitKind::Quota),
    ] {
        limiter.add_limit(
            name.into(),
            Box::new(TestLimit {
                kind,
                state: state.clone(),
            }),
        );
    }
    limiter
}

fn config() -> Config {
    let mut value = serde_json::to_value(Config::default()).unwrap();
    value["ingress"]["timeout"] = serde_json::json!("10ms");
    value["limits"]["daily"] =
        serde_json::json!({"strategy":"quota_tracker", "params":{"limit":100,"period":"day"}});
    serde_json::from_value(value).unwrap()
}

fn runtime(state: &Arc<AtomicU8>) -> Arc<RuntimeSnapshot> {
    let mut runtime = test_support::runtime(RuntimeConfig::from_raw(config()).unwrap());
    Arc::get_mut(&mut runtime).unwrap().core.limiter = limiter(state);
    runtime
}

fn check(point: LimitPoint) -> TrafficLimit<'static> {
    TrafficLimit {
        name: if matches!(point, LimitPoint::Quota) {
            "daily"
        } else {
            "default"
        },
        key: "test",
        scope: "subject",
        cost: None,
    }
}

const POINTS: [LimitPoint; 5] = [
    LimitPoint::GatewayIngress,
    LimitPoint::IamIngress,
    LimitPoint::AdminPrincipal,
    LimitPoint::RateLimit,
    LimitPoint::Quota,
];

#[tokio::test(start_paused = true)]
async fn traffic_checks_apply_allow_and_deny_to_backend_errors_and_deadlines_at_every_point() {
    for point in POINTS {
        for state in [1, 2] {
            let state = Arc::new(AtomicU8::new(state));
            let runtime = runtime(&state);
            for mode in [OnStoreError::Allow, OnStoreError::Deny] {
                let started = tokio::time::Instant::now();
                let result = check_traffic_limit(&runtime, point, mode, check(point)).await;
                assert!(runtime.limit_store_health.is_degraded());
                if state.load(Ordering::Relaxed) == 2 {
                    assert_eq!(started.elapsed(), runtime.ingress.timeout);
                }
                match mode {
                    OnStoreError::Allow => assert!(result.unwrap().is_none()),
                    OnStoreError::Deny => {
                        use axum::response::IntoResponse;
                        let error = result.unwrap_err();
                        assert_eq!(error.code, point.unavailable());
                        let response = error.into_response();
                        assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
                        assert_eq!(response.headers()["retry-after"], "10");
                    }
                }
            }
            state.store(0, Ordering::Relaxed);
            assert!(
                check_traffic_limit(&runtime, point, OnStoreError::Allow, check(point))
                    .await
                    .unwrap()
                    .unwrap()
                    .is_allowed()
            );
            assert!(!runtime.limit_store_health.is_degraded());
        }
    }
}

#[tokio::test]
async fn configuration_and_clock_errors_never_fail_open_or_mark_the_store_degraded() {
    for state in [0, 3, 4] {
        let runtime = runtime(&Arc::new(AtomicU8::new(state)));
        for point in POINTS {
            for mode in [OnStoreError::Allow, OnStoreError::Deny] {
                let cases = [
                    TrafficLimit {
                        name: "missing",
                        ..check(point)
                    },
                    TrafficLimit {
                        name: if matches!(point, LimitPoint::Quota) {
                            "default"
                        } else {
                            "daily"
                        },
                        ..check(point)
                    },
                ];
                for check in cases {
                    let error = check_traffic_limit(&runtime, point, mode, check)
                        .await
                        .unwrap_err();
                    assert_eq!(error.code, ErrorCode::GatewayLimitConfigurationInvalid);
                    assert_eq!(error.status, http::StatusCode::INTERNAL_SERVER_ERROR);
                }
                if state != 0 {
                    let error = check_traffic_limit(&runtime, point, mode, check(point))
                        .await
                        .unwrap_err();
                    assert_eq!(error.code, ErrorCode::GatewayLimitConfigurationInvalid);
                }
            }
        }
        assert!(!runtime.limit_store_health.is_degraded());
    }
}

#[tokio::test]
async fn a_store_denial_is_a_successful_check_and_clears_only_its_own_degraded_point() {
    let state = Arc::new(AtomicU8::new(1));
    let runtime = runtime(&state);
    for point in POINTS {
        check_traffic_limit(&runtime, point, OnStoreError::Allow, check(point))
            .await
            .unwrap();
    }
    state.store(5, Ordering::Relaxed);
    for (index, point) in POINTS.into_iter().enumerate() {
        assert!(
            !check_traffic_limit(&runtime, point, OnStoreError::Allow, check(point))
                .await
                .unwrap()
                .unwrap()
                .is_allowed()
        );
        assert_eq!(runtime.limit_store_health.is_degraded(), index < 4);
    }
}

#[cfg(feature = "memory")]
#[tokio::test]
async fn gateway_ingress_and_route_policies_fail_open_to_a_real_backend_and_fail_closed_with_retry_after()
 {
    use axum::{Router, body::Body, routing::get};
    use http::Request;
    use http_body_util::BodyExt;
    use serde_json::json;
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let backend = Router::new().route(
        "/",
        get({
            let hits = hits.clone();
            move || {
                hits.fetch_add(1, Ordering::Relaxed);
                async { "backend" }
            }
        }),
    );
    let server = tokio::spawn(async { axum::serve(listener, backend).await.unwrap() });
    for point in ["ingress", "rate_limit", "quota"] {
        for state in [1, 2] {
            for mode in ["allow", "deny"] {
                let mut value = serde_json::to_value(config()).unwrap();
                value["ingress"]["on_store_error"] =
                    json!(if point == "ingress" { mode } else { "allow" });
                value["http"] = json!({
                    "upstreams":{"backend":{"targets":[{"url":url}],"load_balancer":{"strategy":"round_robin"}}},
                    "services":{"backend":{"kind":"load_balancer","upstream":"backend"}},
                    "policies":{},
                    "routers":{"all":{"match":{"path":{"prefix":"/"}},"service":"backend","policies":[]}}
                });
                if point != "ingress" {
                    value["http"]["policies"]["traffic"] = json!({"kind":point,"limit":if point == "quota" {"daily"} else {"default"},"on_store_error":mode});
                    value["http"]["routers"]["all"]["policies"] = json!(["traffic"]);
                }
                let state = Arc::new(AtomicU8::new(state));
                let gate = Arc::new(test_support::gate_with_limiter(
                    serde_json::from_value(value).unwrap(),
                    limiter(&state),
                ));
                let mut request = Request::new(Body::empty());
                request.extensions_mut().insert(gate);
                let before = hits.load(Ordering::Relaxed);
                let response = crate::gate::service(request).await.unwrap();
                if mode == "allow" {
                    assert_eq!(response.status(), 200, "{point}");
                    assert!(!response.headers().contains_key("x-ratelimit-limit"));
                    assert!(!response.headers().contains_key("x-quota-limit"));
                    assert_eq!(
                        response.into_body().collect().await.unwrap().to_bytes(),
                        "backend"
                    );
                    assert_eq!(hits.load(Ordering::Relaxed), before + 1);
                } else {
                    assert_eq!(response.status(), 503, "{point}");
                    assert_eq!(response.headers()["retry-after"], "10");
                    assert_eq!(hits.load(Ordering::Relaxed), before);
                }
            }
        }
    }
    server.abort();
}

#[cfg(feature = "memory")]
#[test]
fn degraded_state_survives_configuration_activation() {
    let gate = test_support::gate(config());
    gate.snapshot().limit_store_health.failure(
        LimitPoint::RateLimit,
        OnStoreError::Allow,
        "backend",
    );
    let mut cfg = config();
    cfg.ingress.on_store_error = "deny".into();
    gate.activate(crate::gate::prepare_config(cfg).unwrap())
        .unwrap();
    assert!(gate.snapshot().limit_store_health.is_degraded());
    assert_eq!(gate.snapshot().ingress.on_store_error, OnStoreError::Deny);
}

#[test]
fn store_errors_are_counted_per_outcome_and_logs_only_describe_transitions() {
    const CHILD: &str = "STARGATE_TEST_LIMIT_OBSERVABILITY";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "gate::limits::store::tests::store_errors_are_counted_per_outcome_and_logs_only_describe_transitions", "--nocapture"])
            .env(CHILD, "1").output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    use opentelemetry_sdk::{
        error::OTelSdkResult,
        metrics::{
            SdkMeterProvider, Temporality,
            data::{AggregatedMetrics, MetricData, ResourceMetrics},
            exporter::PushMetricExporter,
        },
    };
    use std::{collections::BTreeMap, io::Write, sync::Mutex, time::Duration};
    type Samples = Arc<Mutex<BTreeMap<(String, String), u64>>>;
    #[derive(Clone)]
    struct Exporter(Samples);
    impl PushMetricExporter for Exporter {
        async fn export(&self, data: &ResourceMetrics) -> OTelSdkResult {
            for metric in data
                .scope_metrics()
                .flat_map(|scope| scope.metrics())
                .filter(|metric| metric.name() == "stargate.limits.store_errors")
            {
                let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() else {
                    panic!("counter must be a u64 sum")
                };
                for sample in sum.data_points() {
                    let attrs: BTreeMap<_, _> = sample
                        .attributes()
                        .map(|kv| (kv.key.as_str(), kv.value.to_string()))
                        .collect();
                    assert_eq!(attrs.len(), 2);
                    self.0.lock().unwrap().insert(
                        (attrs["point"].clone(), attrs["outcome"].clone()),
                        sample.value(),
                    );
                }
            }
            Ok(())
        }
        fn force_flush(&self) -> OTelSdkResult {
            Ok(())
        }
        fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
            Ok(())
        }
        fn temporality(&self) -> Temporality {
            Temporality::Cumulative
        }
    }
    #[derive(Clone)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let samples = Arc::default();
    let provider = SdkMeterProvider::builder()
        .with_periodic_exporter(Exporter(Arc::clone(&samples)))
        .build();
    opentelemetry::global::set_meter_provider(provider.clone());
    let logs = Arc::new(Mutex::new(Vec::new()));
    let output = logs.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer(move || Capture(output.clone()))
            .finish(),
    )
    .unwrap();
    let health = Arc::new(LimitStoreHealth::default());
    for point in POINTS {
        // Concurrent failures may race on the first transition; exactly one wins.
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let health = &health;
                scope.spawn(move || {
                    for _ in 0..8 {
                        health.failure(point, OnStoreError::Allow, "backend");
                        health.failure(point, OnStoreError::Deny, "timeout");
                    }
                });
            }
        });
        health.success(point);
        health.success(point);
    }
    for code in [
        ErrorCode::GatewayIngressUnavailable,
        ErrorCode::GatewayLimiterUnavailable,
        ErrorCode::IamLimiterUnavailable,
    ] {
        use axum::response::IntoResponse;
        let _ = ErrorResponse::new(code)
            .with_param("point", "test")
            .into_response();
    }
    provider.force_flush().unwrap();
    let samples = samples.lock().unwrap();
    assert_eq!(samples.len(), 10);
    for point in POINTS {
        for outcome in ["allowed", "denied"] {
            assert_eq!(samples[&(point.name().to_owned(), outcome.to_owned())], 64);
        }
    }
    let logs = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    let logs: Vec<serde_json::Value> = logs
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(logs.len(), 10, "{logs:?}");
    for point in POINTS {
        let entries: Vec<_> = logs
            .iter()
            .filter(|log| log["fields"]["point"] == point.name())
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["level"], "WARN");
        assert_eq!(entries[1]["level"], "INFO");
    }
    assert!(!health.is_degraded());
}
