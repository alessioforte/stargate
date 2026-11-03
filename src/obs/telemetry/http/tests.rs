use super::*;
use crate::obs::telemetry;
use axum::{Router, body::Body, middleware::from_fn, routing::any};
use http::{Request, StatusCode};
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::{
    error::OTelSdkResult,
    metrics::{
        SdkMeterProvider, Temporality,
        data::{AggregatedMetrics, MetricData, ResourceMetrics},
        exporter::PushMetricExporter,
    },
    trace::{SdkTracerProvider, SpanData, SpanExporter},
};
use std::{collections::BTreeMap, io::Write, sync::Mutex, time::Duration};
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;

#[derive(Debug, Clone, Default)]
struct TraceCapture(Arc<Mutex<Vec<SpanData>>>);
impl SpanExporter for TraceCapture {
    async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        self.0.lock().unwrap().extend(batch);
        Ok(())
    }
}
#[derive(Debug, Clone)]
struct Sample {
    name: String,
    unit: String,
    attrs: BTreeMap<String, String>,
    bounds: Vec<f64>,
    sum: f64,
}
type InstrumentAttributes = BTreeMap<String, Vec<BTreeMap<String, String>>>;

#[derive(Debug, Clone, Default)]
struct MetricCapture(Arc<Mutex<Vec<Sample>>>, Arc<Mutex<InstrumentAttributes>>);

fn attribute_sets<T>(data: &MetricData<T>) -> Vec<BTreeMap<String, String>> {
    match data {
        MetricData::Gauge(gauge) => gauge
            .data_points()
            .map(|p| attributes(p.attributes()))
            .collect(),
        MetricData::Sum(sum) => sum
            .data_points()
            .map(|p| attributes(p.attributes()))
            .collect(),
        MetricData::Histogram(histogram) => histogram
            .data_points()
            .map(|p| attributes(p.attributes()))
            .collect(),
        MetricData::ExponentialHistogram(histogram) => histogram
            .data_points()
            .map(|p| attributes(p.attributes()))
            .collect(),
    }
}
impl PushMetricExporter for MetricCapture {
    async fn export(&self, metrics: &ResourceMetrics) -> OTelSdkResult {
        let mut samples = self.0.lock().unwrap();
        for scope in metrics.scope_metrics() {
            for metric in scope.metrics() {
                let attrs = match metric.data() {
                    AggregatedMetrics::F64(data) => attribute_sets(data),
                    AggregatedMetrics::U64(data) => attribute_sets(data),
                    AggregatedMetrics::I64(data) => attribute_sets(data),
                };
                self.1
                    .lock()
                    .unwrap()
                    .entry(metric.name().to_owned())
                    .or_default()
                    .extend(attrs);
                if let AggregatedMetrics::F64(MetricData::Histogram(histogram)) = metric.data() {
                    for point in histogram.data_points() {
                        samples.push(Sample {
                            name: metric.name().into(),
                            unit: metric.unit().into(),
                            attrs: attributes(point.attributes()),
                            bounds: point.bounds().collect(),
                            sum: point.sum(),
                        });
                    }
                }
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
struct LogCapture(Arc<Mutex<Vec<u8>>>);
impl Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn attributes<'a>(attrs: impl Iterator<Item = &'a KeyValue>) -> BTreeMap<String, String> {
    attrs
        .map(|kv| (kv.key.as_str().into(), kv.value.to_string()))
        .collect()
}
async fn send(app: &Router, method: &str, path: &str) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "identity.example:8443")
        .header("user-agent", "test-agent")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(axum::extract::ConnectInfo(
        "203.0.113.9:4444".parse::<std::net::SocketAddr>().unwrap(),
    ));
    app.clone().oneshot(req).await.unwrap();
}

fn emit_metric_samples() {
    // Exercise each duration instrument with a known conversion, including sub-ms signing.
    use crate::obs::telemetry;
    telemetry::record_gateway_request("test", "test", 200, Duration::from_millis(250));
    telemetry::record_gateway_upstream_attempt(
        "test",
        "upstream",
        "http",
        Some(200),
        "success",
        Duration::from_millis(250),
    );
    telemetry::record_gateway_transfer("total", "success", Duration::from_millis(250));
    telemetry::record_gateway_response_disposal("test", "success", 0, Duration::from_millis(250));
    telemetry::record_internal_context_issue(
        "test",
        "primary",
        "success",
        "none",
        Some(Duration::from_micros(10)),
        Some(1),
    );
    telemetry::record_internal_context_queue("test", "primary", Duration::from_micros(10));
    #[cfg(all(feature = "postgres", feature = "redis"))]
    telemetry::record_audit_relay_batch("success", 0, Duration::from_millis(250));

    telemetry::record_gateway_grpc_response("test", "test", 14);
    telemetry::record_gateway_resource("primary", 1);
    telemetry::record_gateway_rejection("primary");
    telemetry::record_gateway_timeout("total");
    telemetry::record_limit_store_error("quota", "denied");
    telemetry::record_config_reload("gateway_config", "success");
    telemetry::record_gateway_failover("status", Some(503));
    telemetry::record_gateway_mirror("success");
    telemetry::record_gateway_replay_bytes(1);
    telemetry::record_gateway_policy("auth", "allowed");
    telemetry::record_propagation_draft("success", "user");
}

#[test]
fn all_metric_attribute_keys_follow_the_semantic_namespace_contract() {
    const CHILD: &str = "STARGATE_TEST_METRIC_NAMESPACE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "obs::telemetry::http::tests::all_metric_attribute_keys_follow_the_semantic_namespace_contract", "--nocapture"])
            .env(CHILD, "1").output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let capture = MetricCapture::default();
    let provider = SdkMeterProvider::builder()
        .with_periodic_exporter(capture.clone())
        .build();
    opentelemetry::global::set_meter_provider(provider.clone());
    emit_metric_samples();
    let request = HttpRequestTelemetry::new(tracing::Span::none(), normalized_method("GET"));
    request.record_route("/metric/{id}");
    crate::obs::telemetry::record_http_server_request(
        crate::obs::telemetry::HttpRequestKind::Iam,
        &request,
        200,
        "http",
        Duration::from_millis(1),
    );
    provider.force_flush().unwrap();
    let instruments = capture.1.lock().unwrap();
    assert!(instruments.len() >= 20);
    let standard = [
        "http.request.method",
        "http.response.status_code",
        "http.route",
        "url.scheme",
        "error.type",
        "network.protocol.name",
        "rpc.status_code",
    ];
    for (name, sets) in instruments.iter() {
        for attrs in sets {
            for key in attrs.keys() {
                assert!(
                    key.starts_with("stargate.") || standard.contains(&key.as_str()),
                    "{name}: {key}"
                );
            }
        }
    }
    for (instrument, key) in [
        ("stargate.limits.store_errors", "stargate.point"),
        ("stargate.limits.store_errors", "stargate.outcome"),
        ("stargate.gateway.grpc.responses", "stargate.router"),
        ("stargate.gateway.grpc.responses", "stargate.service"),
        ("stargate.gateway.grpc.responses", "rpc.status_code"),
        ("stargate.gateway.resources.active", "stargate.kind"),
        ("stargate.gateway.rejections", "stargate.kind"),
        ("stargate.gateway.timeouts", "stargate.phase"),
        ("stargate.gateway.transfer.duration", "stargate.phase"),
    ] {
        assert!(
            instruments[instrument]
                .iter()
                .any(|attrs| attrs.contains_key(key)),
            "{instrument}: {key}"
        );
    }
    assert!(
        instruments["stargate.gateway.grpc.responses"]
            .iter()
            .any(|attrs| attrs["rpc.status_code"] == "UNAVAILABLE")
    );
}

#[tokio::test]
async fn server_spans_and_metrics_follow_http_conventions_without_query_values() {
    const CHILD: &str = "STARGATE_TEST_HTTP_SEMCONV_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "obs::telemetry::http::tests::server_spans_and_metrics_follow_http_conventions_without_query_values", "--nocapture"])
            .env(CHILD,"1").output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let traces = TraceCapture::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(traces.clone())
        .build();
    let metrics = MetricCapture::default();
    let meter = SdkMeterProvider::builder()
        .with_periodic_exporter(metrics.clone())
        .build();
    opentelemetry::global::set_meter_provider(meter.clone());
    let logs = Arc::new(Mutex::new(Vec::new()));
    let writer = LogCapture(logs.clone());
    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_ansi(false)
                .with_writer(move || writer.clone()),
        )
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("http-contract-test")));
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let app = Router::new()
        .route(
            "/admin/users/{id}",
            any(|| async {
                tracing::info!("request_handler");
                StatusCode::OK
            }),
        )
        .route(
            "/failure",
            any(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        )
        .route("/denied", any(|| async { StatusCode::FORBIDDEN }))
        .route_layer(from_fn(crate::api::middleware::telemetry::record_route))
        .fallback(|| async { StatusCode::NOT_FOUND })
        .layer(from_fn(crate::obs::logging::trace_middleware));
    send(
        &app,
        "GET",
        "/admin/users/one?code=private-code&state=private-state&%63ode=encoded-secret",
    )
    .await;
    send(&app, "GET", "/admin/users/two").await;
    send(&app, "CUSTOM_METHOD", "/admin/users/three").await;
    send(&app, "GET", "/failure").await;
    send(&app, "GET", "/denied").await;
    send(&app, "GET", "/missing").await;
    let probes = crate::api::health::router(crate::srv::health::Lifecycle::default())
        .layer(from_fn(crate::obs::logging::trace_middleware));
    send(&probes, "GET", "/livez").await;

    #[cfg(feature = "memory")]
    {
        let mut config = serde_json::to_value(gate::cfg::Config::default()).unwrap();
        config["http"] = serde_json::json!({"services":{"ok":{"kind":"direct_response","status":200}},
            "routers":{"template":{"match":{"path":{"template":"/objects/{id}"}},"service":"ok"},
            "prefix":{"match":{"path":{"prefix":"/prefix"}},"service":"ok"},
            "exact":{"match":{"path":{"exact":"/exact"}},"service":"ok"}}});
        let gate = Arc::new(crate::gate::test_support::gate(
            serde_json::from_value(config).unwrap(),
        ));
        let app = Router::new()
            .fallback(|req: axum::extract::Request| async {
                crate::gate::service(req).await.unwrap()
            })
            .layer(axum::Extension(gate))
            .layer(from_fn(crate::obs::logging::trace_middleware));
        for path in [
            "/objects/one",
            "/objects/two",
            "/prefix/one",
            "/exact",
            "/unmatched",
        ] {
            send(&app, "GET", path).await;
        }
    }
    emit_metric_samples();
    let manual = HttpRequestTelemetry::new(
        tracing::Span::none(),
        normalized_method("ANOTHER_UNKNOWN_METHOD"),
    );
    manual.record_route("/metric/{id}");
    manual.record_error("503");
    telemetry::record_http_server_request(
        telemetry::HttpRequestKind::Iam,
        &manual,
        503,
        "http",
        Duration::from_millis(500),
    );
    meter.force_flush().unwrap();
    provider.force_flush().unwrap();
    let spans = traces.0.lock().unwrap();
    let server: Vec<_> = spans
        .iter()
        .filter(|span| span.span_kind == opentelemetry::trace::SpanKind::Server)
        .collect();
    assert!(server.len() >= 7);
    let users: Vec<_> = server
        .iter()
        .filter(|span| span.name == "GET /admin/users/{id}")
        .collect();
    assert_eq!(
        users.len(),
        2,
        "{:?}",
        server
            .iter()
            .map(|s| (
                &s.name,
                attributes(s.attributes.iter()).get("http.route").cloned()
            ))
            .collect::<Vec<_>>()
    ); // otel.name recorded after creation changes the exported name.
    let unknown = server
        .iter()
        .find(|span| {
            attributes(span.attributes.iter())
                .get("http.request.method_original")
                .is_some_and(|v| v == "CUSTOM_METHOD")
        })
        .unwrap();
    assert_eq!(unknown.name, "HTTP /admin/users/{id}");
    assert_eq!(
        attributes(unknown.attributes.iter())["http.request.method"],
        "_OTHER"
    );
    for span in &server {
        let attrs = attributes(span.attributes.iter());
        for old in [
            "http.method",
            "http.path",
            "http.query",
            "url.query",
            "http.user_agent",
            "http.peer_ip",
            "http.client_ip",
            "http.status_code",
        ] {
            assert!(!attrs.contains_key(old), "{old}");
        }
        let status = attrs["http.response.status_code"].parse::<u16>().unwrap();
        assert_eq!(
            matches!(span.status, opentelemetry::trace::Status::Error { .. }),
            status >= 500
        );
        assert_eq!(attrs.contains_key("error.type"), status >= 500);
        assert_eq!(attrs["server.address"], "identity.example");
        assert_eq!(attrs["server.port"], "8443");
        assert_eq!(attrs["client.address"], "203.0.113.9");
        assert_eq!(attrs["network.peer.address"], "203.0.113.9");
        assert_eq!(attrs["user_agent.original"], "test-agent");
        assert!(attrs.contains_key("request_id"));
        if attrs["url.path"] == "/missing" {
            assert_eq!(span.name, "GET");
            assert!(!attrs.contains_key("http.route"));
        }
        if attrs["url.path"] == "/livez" {
            assert_eq!(span.name, "GET /livez");
        }
        #[cfg(feature = "memory")]
        if let Some(router) = attrs.get("stargate.router") {
            match router.as_str() {
                "template" => {
                    assert_eq!(span.name, "GET /objects/{id}");
                    assert_eq!(attrs["http.route"], "/objects/{id}");
                }
                "prefix" => {
                    assert_eq!(span.name, "GET");
                    assert!(!attrs.contains_key("http.route"));
                }
                "exact" => {
                    assert_eq!(span.name, "GET /exact");
                }
                _ => {}
            }
        }
    }
    let text = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    for secret in [
        "private-code",
        "private-state",
        "encoded-secret",
        "http.query",
        "url.query",
    ] {
        assert!(!text.contains(secret), "{secret}");
    }
    for line in text.lines() {
        serde_json::from_str::<serde_json::Value>(line).unwrap();
    }
    let samples = metrics.0.lock().unwrap();
    for sample in samples.iter() {
        assert_eq!(sample.unit, "s", "{}", sample.name);
        assert!(!sample.bounds.is_empty());
        assert!(!sample.name.starts_with("stargate.http.server"));
        if sample.name == "http.server.request.duration" {
            assert_eq!(sample.bounds, super::super::metrics::HTTP_DURATION_BUCKETS);
            assert!(
                !sample
                    .attrs
                    .values()
                    .any(|v| v.contains("CUSTOM_METHOD") || v.contains("ANOTHER_UNKNOWN_METHOD"))
            );
            assert!(sample.attrs.contains_key("url.scheme"));
            assert!(!sample.attrs.contains_key("url.path"));
        }
    }
    assert!(samples.iter().any(|sample| {
        sample.name == "http.server.request.duration"
            && sample
                .attrs
                .get("http.route")
                .is_some_and(|route| route == "/metric/{id}")
            && sample.attrs["http.request.method"] == "_OTHER"
            && sample.sum == 0.5
    }));
    for name in [
        "stargate.gateway.duration",
        "stargate.gateway.transfer.duration",
        "stargate.gateway.upstream.duration",
        "stargate.gateway.response.disposal.duration",
    ] {
        assert!(samples.iter().any(|s| s.name == name && s.sum >= 0.25));
    }
    for name in [
        "stargate.gateway.internal_context.signing.duration",
        "stargate.gateway.internal_context.queue.duration",
    ] {
        assert!(samples.iter().any(|s| s.name == name
            && s.sum == 0.00001
            && s.bounds.iter().any(|bound| *bound < 0.001)));
    }
}
