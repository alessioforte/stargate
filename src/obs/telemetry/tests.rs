use axum::{Router, body::Bytes, extract::State, routing::post};
use http::{HeaderMap, Uri};
use std::{
    process::Command,
    sync::{Arc, Mutex},
};
use tracing::Instrument;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_export_workers_deliver_traces_and_logs_without_a_tokio_runtime() {
    const CHILD: &str = "STARGATE_TEST_HTTP_EXPORT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let guard = crate::obs::logging::init(crate::obs::logging::Settings::from_env().unwrap());
        let span = tracing::info_span!("real_http_export");
        let mut headers = HeaderMap::new();
        headers.insert(
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
                .parse()
                .unwrap(),
        );
        super::set_span_parent_from_headers(&span, &headers);
        async {
            tracing::info!("real_http_export_log");
            opentelemetry::global::meter("http_export_test")
                .u64_counter("http_export_requests")
                .build()
                .add(1, &[]);
        }
        .instrument(span)
        .await;
        guard.shutdown();
        return;
    }

    struct Export {
        path: String,
        body: Bytes,
    }
    let exports = Arc::new(Mutex::new(Vec::<Export>::new()));
    let app = Router::new()
        .route(
            "/v1/{signal}",
            post(
                |State(exports): State<Arc<Mutex<Vec<Export>>>>, uri: Uri, body: Bytes| async move {
                    exports.lock().unwrap().push(Export {
                        path: uri.path().to_owned(),
                        body,
                    });
                    ([("content-type", "application/x-protobuf")], Bytes::new())
                },
            ),
        )
        .with_state(exports.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let output = tokio::task::spawn_blocking(move || {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "obs::telemetry::tests::http_export_workers_deliver_traces_and_logs_without_a_tokio_runtime", "--nocapture"])
            .env(CHILD, "1")
            .env("OTEL_ENABLED", "true")
            .env("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf")
            .env("OTEL_EXPORTER_OTLP_ENDPOINT", endpoint)
            .env("OTEL_TRACES_SAMPLER", "always_on")
            .env("OTEL_RESOURCE_ATTRIBUTES", "service.instance.id=http-export-test-replica,custom.test.attribute=preserved")
            .env("OTEL_EXPORTER_OTLP_TIMEOUT", "3000")
            .env("LOG_FORMAT", "json")
            .env("LOG_FILE_ENABLED", "false")
            .env("LOG_CONSOLE_ENABLED", "true")
            .env("RUST_LOG", "info");
        for signal in ["TRACES", "LOGS", "METRICS"] {
            for setting in ["ENDPOINT", "PROTOCOL", "HEADERS", "TIMEOUT"] {
                command.env_remove(format!("OTEL_EXPORTER_OTLP_{signal}_{setting}"));
            }
        }
        command.env_remove("OTEL_EXPORTER_OTLP_HEADERS");
        command.output().unwrap()
    }).await.unwrap();
    server.abort();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{}\n{stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!stderr.contains("panicked"), "{stderr}");
    let exports = exports.lock().unwrap();
    for signal in ["traces", "metrics", "logs"] {
        let path = format!("/v1/{signal}");
        assert!(
            exports.iter().any(|export| export.path == path
                && export
                    .body
                    .windows(b"service.instance.id".len())
                    .any(|value| value == b"service.instance.id")
                && export
                    .body
                    .windows(b"http-export-test-replica".len())
                    .any(|value| value == b"http-export-test-replica")
                && export
                    .body
                    .windows(b"custom.test.attribute".len())
                    .any(|value| value == b"custom.test.attribute")),
            "Missing resource identity in {signal}"
        );
    }
    let trace_id = [
        0x4b, 0xf9, 0x2f, 0x35, 0x77, 0xb3, 0x4d, 0xa6, 0xa3, 0xce, 0x92, 0x9d, 0x0e, 0x0e, 0x47,
        0x36,
    ];
    assert!(
        exports
            .iter()
            .any(|e| e.path == "/v1/traces" && e.body.windows(16).any(|w| w == trace_id))
    );
    assert!(exports.iter().any(|e| {
        e.path == "/v1/logs"
            && e.body
                .windows(b"real_http_export_log".len())
                .any(|w| w == b"real_http_export_log")
    }));
    assert!(
        exports
            .iter()
            .any(|e| e.path == "/v1/metrics" && !e.body.is_empty())
    );
}

#[test]
fn resource_identity_is_stable_per_process_and_preserves_environment_attributes() {
    const CHILD: &str = "STARGATE_TEST_RESOURCE_IDENTITY_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let resource = super::resource();
        let key = opentelemetry::Key::from_static_str("service.instance.id");
        let id = resource.get(&key).unwrap().to_string();
        assert_eq!(super::resource().get(&key).unwrap().to_string(), id);
        let attributes: std::collections::BTreeMap<_, _> = resource
            .iter()
            .map(|(key, value)| (key.as_str().to_string(), value.to_string()))
            .collect();
        println!(
            "RESOURCE_IDENTITY={}",
            serde_json::to_string(&attributes).unwrap()
        );
        return;
    }
    fn child(attributes: Option<&str>) -> serde_json::Value {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "obs::telemetry::tests::resource_identity_is_stable_per_process_and_preserves_environment_attributes", "--nocapture"])
            .env(CHILD, "1").env_remove("OTEL_RESOURCE_ATTRIBUTES").env_remove("OTEL_SERVICE_NAME");
        if let Some(attributes) = attributes {
            command.env("OTEL_RESOURCE_ATTRIBUTES", attributes);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let json = stdout
            .lines()
            .find_map(|line| line.strip_prefix("RESOURCE_IDENTITY="))
            .unwrap();
        serde_json::from_str(json).unwrap()
    }
    let first = child(None);
    let second = child(None);
    for resource in [&first, &second] {
        assert!(
            resource["service.instance.id"]
                .as_str()
                .unwrap()
                .parse::<ulid::Ulid>()
                .is_ok()
        );
        assert_eq!(resource["service.name"], "stargate");
        assert_eq!(resource["service.version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(
            resource["deployment.profile"],
            crate::srv::profile::COMPILED_PROFILE
        );
        assert!(resource["telemetry.sdk.name"].is_string());
    }
    assert_ne!(first["service.instance.id"], second["service.instance.id"]);
    let explicit = child(Some(
        "service.instance.id=replica-1,k8s.namespace.name=identity,custom.attribute=preserved",
    ));
    assert_eq!(explicit["service.instance.id"], "replica-1");
    assert_eq!(explicit["k8s.namespace.name"], "identity");
    assert_eq!(explicit["custom.attribute"], "preserved");
    let blank = child(Some("service.instance.id=,custom.attribute=preserved"));
    assert!(
        blank["service.instance.id"]
            .as_str()
            .unwrap()
            .parse::<ulid::Ulid>()
            .is_ok()
    );
    assert_eq!(blank["custom.attribute"], "preserved");
}

/// Reproducible instrument-contract probe for an external OTLP collector.
/// Run only this test in a dedicated process with OTEL_* pointing to that collector.
#[tokio::test]
#[ignore = "requires a dedicated OTLP collector and STARGATE_TEST_MONITORING_EXPORT=1"]
async fn export_monitoring_instruments_to_a_real_collector() {
    assert_eq!(
        std::env::var("STARGATE_TEST_MONITORING_EXPORT").as_deref(),
        Ok("1")
    );
    let guard = super::init();
    assert!(guard.enabled());
    use std::time::Duration;
    let request = super::HttpRequestTelemetry::new(tracing::Span::none(), "GET");
    request.record_route("/example");
    request.record_error("503");
    super::record_http_server_request(
        super::HttpRequestKind::Iam,
        &request,
        503,
        "http",
        Duration::from_millis(10),
    );
    super::record_gateway_request("example", "backend", 503, Duration::from_millis(5));
    super::record_gateway_upstream_attempt(
        "backend",
        "upstream",
        "http",
        Some(503),
        "error",
        Duration::from_millis(4),
    );
    super::record_gateway_failover("status", Some(503));
    super::record_gateway_rejection("primary");
    super::record_gateway_resource("primary", 1);
    super::record_gateway_resource("replay_memory", 1024);
    super::record_limit_store_error("gateway_ingress", "denied");
    super::record_config_reload("gateway_config", "transport_error");
    #[cfg(all(feature = "postgres", feature = "redis"))]
    super::record_audit_relay_batch("error", 0, Duration::from_millis(1));
    guard.shutdown();
}
