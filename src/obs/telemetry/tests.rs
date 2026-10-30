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
