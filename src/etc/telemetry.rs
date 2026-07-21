use crate::etc::env::bool_or;
use http::{HeaderMap, HeaderName, HeaderValue};
use once_cell::sync::Lazy;
use opentelemetry::{
    Context, KeyValue, global,
    metrics::{Counter, Histogram},
    propagation::{Extractor, Injector, TextMapCompositePropagator},
    trace::TraceContextExt,
};
use opentelemetry_sdk::{
    Resource,
    logs::SdkLoggerProvider,
    metrics::SdkMeterProvider,
    propagation::{BaggagePropagator, TraceContextPropagator},
    trace::SdkTracerProvider,
};
use std::time::Duration;
use tracing_opentelemetry::OpenTelemetrySpanExt;

#[cfg(test)]
use opentelemetry::propagation::TextMapPropagator as _;

const INSTRUMENTATION_NAME: &str = "stargate";

pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl TelemetryGuard {
    pub fn disabled() -> Self {
        Self {
            tracer_provider: None,
            meter_provider: None,
            logger_provider: None,
        }
    }

    pub fn enabled(&self) -> bool {
        self.tracer_provider.is_some()
            || self.meter_provider.is_some()
            || self.logger_provider.is_some()
    }

    pub fn tracer_provider(&self) -> Option<&SdkTracerProvider> {
        self.tracer_provider.as_ref()
    }

    pub fn logger_provider(&self) -> Option<&SdkLoggerProvider> {
        self.logger_provider.as_ref()
    }

    pub fn shutdown(&self) {
        if let Some(provider) = &self.tracer_provider
            && let Err(error) = provider.shutdown()
        {
            tracing::warn!(%error, "OpenTelemetry tracer provider shutdown failed");
        }
        if let Some(provider) = &self.meter_provider
            && let Err(error) = provider.shutdown()
        {
            tracing::warn!(%error, "OpenTelemetry meter provider shutdown failed");
        }
        if let Some(provider) = &self.logger_provider
            && let Err(error) = provider.shutdown()
        {
            tracing::warn!(%error, "OpenTelemetry logger provider shutdown failed");
        }
    }
}

pub fn init() -> TelemetryGuard {
    if !otel_enabled() {
        return TelemetryGuard::disabled();
    }

    global::set_text_map_propagator(TextMapCompositePropagator::new(vec![
        Box::new(TraceContextPropagator::new()),
        Box::new(BaggagePropagator::new()),
    ]));

    let resource = resource();

    let tracer_provider = match opentelemetry_otlp::SpanExporter::builder().build() {
        Ok(exporter) => {
            let provider = SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource.clone())
                .build();
            global::set_tracer_provider(provider.clone());
            Some(provider)
        }
        Err(error) => {
            tracing::warn!(%error, "OpenTelemetry trace exporter disabled");
            None
        }
    };

    let meter_provider = match opentelemetry_otlp::MetricExporter::builder().build() {
        Ok(exporter) => {
            let provider = SdkMeterProvider::builder()
                .with_periodic_exporter(exporter)
                .with_resource(resource.clone())
                .build();
            global::set_meter_provider(provider.clone());
            Some(provider)
        }
        Err(error) => {
            tracing::warn!(%error, "OpenTelemetry metric exporter disabled");
            None
        }
    };

    let logger_provider = match opentelemetry_otlp::LogExporter::builder().build() {
        Ok(exporter) => Some(
            SdkLoggerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource)
                .build(),
        ),
        Err(error) => {
            tracing::warn!(%error, "OpenTelemetry log exporter disabled");
            None
        }
    };

    TelemetryGuard {
        tracer_provider,
        meter_provider,
        logger_provider,
    }
}

fn otel_enabled() -> bool {
    let default = [
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
        "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
        "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
    ]
    .iter()
    .any(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()));

    bool_or("OTEL_ENABLED", default)
}

fn resource() -> Resource {
    let service_name =
        std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "stargate".to_string());

    Resource::builder()
        .with_service_name(service_name)
        .with_attributes([
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new("deployment.profile", crate::etc::profile::COMPILED_PROFILE),
            KeyValue::new("db.system", crate::etc::profile::COMPILED_DB_BACKEND),
            KeyValue::new(
                "stargate.state_backend",
                crate::etc::profile::COMPILED_STATE_BACKEND,
            ),
        ])
        .build()
}

pub fn extract_context(headers: &HeaderMap) -> Context {
    global::get_text_map_propagator(|propagator| propagator.extract(&HeaderExtractor(headers)))
}

/// Return only a syntactically valid W3C trace id. Audit request facts do not
/// copy the raw `traceparent` header.
#[cfg(test)]
pub fn trace_id_from_headers(headers: &HeaderMap) -> Option<String> {
    let context = TraceContextPropagator::new().extract(&HeaderExtractor(headers));
    let span = context.span();
    let span_context = span.span_context();
    span_context
        .is_valid()
        .then(|| span_context.trace_id().to_string())
}

pub fn set_span_parent_from_headers(span: &tracing::Span, headers: &HeaderMap) {
    let parent = extract_context(headers);
    if let Err(error) = span.set_parent(parent) {
        tracing::trace!(%error, "OpenTelemetry parent context was not attached to span");
    }
}

/// Return the trace id assigned to a tracing span by the OpenTelemetry layer.
/// This captures a newly-created server trace when no valid parent was sent.
pub fn trace_id_from_span(span: &tracing::Span) -> Option<String> {
    let context = span.context();
    let otel_span = context.span();
    let span_context = otel_span.span_context();
    span_context
        .is_valid()
        .then(|| span_context.trace_id().to_string())
}

pub fn inject_context(headers: &mut HeaderMap) {
    let context = tracing::Span::current().context();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&context, &mut HeaderInjector(headers));
    });
}

struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        let name = HeaderName::from_bytes(key.as_bytes()).ok()?;
        self.0.get(name).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(HeaderName::as_str).collect()
    }
}

struct HeaderInjector<'a>(&'a mut HeaderMap);

impl Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        let Ok(name) = HeaderName::from_bytes(key.as_bytes()) else {
            return;
        };
        let Ok(value) = HeaderValue::from_str(&value) else {
            return;
        };
        self.0.insert(name, value);
    }

    fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }
}

pub struct Metrics {
    http_server_requests: Counter<u64>,
    http_server_duration: Histogram<f64>,
    gateway_requests: Counter<u64>,
    gateway_duration: Histogram<f64>,
    gateway_upstream_requests: Counter<u64>,
    gateway_upstream_duration: Histogram<f64>,
    gateway_failovers: Counter<u64>,
    gateway_mirrors: Counter<u64>,
    gateway_replay_bytes: Histogram<u64>,
    gateway_policy_decisions: Counter<u64>,
    gateway_propagation_drafts: Counter<u64>,
    // The audit relay only runs in cluster deployments (postgres + redis).
    #[cfg(all(feature = "postgres", feature = "redis"))]
    audit_relay_records: Counter<u64>,
    #[cfg(all(feature = "postgres", feature = "redis"))]
    audit_relay_duration: Histogram<f64>,
    #[cfg(all(feature = "postgres", feature = "redis"))]
    audit_relay_errors: Counter<u64>,
    config_reloads: Counter<u64>,
}

static METRICS: Lazy<Metrics> = Lazy::new(|| {
    let meter = global::meter(INSTRUMENTATION_NAME);
    Metrics {
        http_server_requests: meter
            .u64_counter("stargate.http.server.requests")
            .with_description("Inbound HTTP requests handled by Stargate")
            .build(),
        http_server_duration: meter
            .f64_histogram("stargate.http.server.duration")
            .with_description("Inbound HTTP request duration")
            .with_unit("ms")
            .build(),
        gateway_requests: meter
            .u64_counter("stargate.gateway.requests")
            .with_description("Requests handled by the gateway fallback")
            .build(),
        gateway_duration: meter
            .f64_histogram("stargate.gateway.duration")
            .with_description("Gateway request duration")
            .with_unit("ms")
            .build(),
        gateway_upstream_requests: meter
            .u64_counter("stargate.gateway.upstream.requests")
            .with_description("Gateway upstream or direct-response attempts")
            .build(),
        gateway_upstream_duration: meter
            .f64_histogram("stargate.gateway.upstream.duration")
            .with_description("Gateway upstream or direct-response attempt duration")
            .with_unit("ms")
            .build(),
        gateway_failovers: meter
            .u64_counter("stargate.gateway.failovers")
            .with_description("Gateway failover attempts")
            .build(),
        gateway_mirrors: meter
            .u64_counter("stargate.gateway.mirrors")
            .with_description("Gateway mirror dispatches")
            .build(),
        gateway_replay_bytes: meter
            .u64_histogram("stargate.gateway.replay.bytes")
            .with_description("Gateway replay-buffered request body size")
            .with_unit("By")
            .build(),
        gateway_policy_decisions: meter
            .u64_counter("stargate.gateway.policy.decisions")
            .with_description("Gateway policy decisions")
            .build(),
        gateway_propagation_drafts: meter
            .u64_counter("stargate.gateway.propagation.drafts")
            .with_description("Trusted internal-context draft construction outcomes")
            .build(),
        #[cfg(all(feature = "postgres", feature = "redis"))]
        audit_relay_records: meter
            .u64_counter("stargate.audit.relay.records")
            .with_description("Audit outbox records relayed")
            .build(),
        #[cfg(all(feature = "postgres", feature = "redis"))]
        audit_relay_duration: meter
            .f64_histogram("stargate.audit.relay.duration")
            .with_description("Audit relay batch duration")
            .with_unit("ms")
            .build(),
        #[cfg(all(feature = "postgres", feature = "redis"))]
        audit_relay_errors: meter
            .u64_counter("stargate.audit.relay.errors")
            .with_description("Audit relay batch failures")
            .build(),
        config_reloads: meter
            .u64_counter("stargate.config.reloads")
            .with_description("Gateway config or policy reload outcomes")
            .build(),
    }
});

pub fn record_http_server_request(method: &str, status: u16, elapsed: Duration) {
    let attrs = [
        KeyValue::new("http.request.method", method.to_string()),
        KeyValue::new("http.response.status_code", i64::from(status)),
    ];
    METRICS.http_server_requests.add(1, &attrs);
    METRICS
        .http_server_duration
        .record(duration_ms(elapsed), &attrs);
}

pub fn record_gateway_request(router: &str, service: &str, status: u16, elapsed: Duration) {
    let attrs = [
        KeyValue::new("stargate.router", router.to_string()),
        KeyValue::new("stargate.service", service.to_string()),
        KeyValue::new("http.response.status_code", i64::from(status)),
    ];
    METRICS.gateway_requests.add(1, &attrs);
    METRICS
        .gateway_duration
        .record(duration_ms(elapsed), &attrs);
}

pub fn record_gateway_upstream_attempt(
    service: &str,
    target_kind: &str,
    protocol: &str,
    status: Option<u16>,
    outcome: &str,
    elapsed: Duration,
) {
    let attrs = [
        KeyValue::new("stargate.service", service.to_string()),
        KeyValue::new("stargate.target_kind", target_kind.to_string()),
        KeyValue::new("network.protocol.name", protocol.to_string()),
        KeyValue::new(
            "http.response.status_code",
            status.map(i64::from).unwrap_or(0),
        ),
        KeyValue::new("stargate.outcome", outcome.to_string()),
    ];
    METRICS.gateway_upstream_requests.add(1, &attrs);
    METRICS
        .gateway_upstream_duration
        .record(duration_ms(elapsed), &attrs);
}

pub fn record_gateway_failover(reason: &str, status: Option<u16>) {
    METRICS.gateway_failovers.add(
        1,
        &[
            KeyValue::new("stargate.reason", reason.to_string()),
            KeyValue::new(
                "http.response.status_code",
                status.map(i64::from).unwrap_or(0),
            ),
        ],
    );
}

pub fn record_gateway_mirror(outcome: &str) {
    METRICS
        .gateway_mirrors
        .add(1, &[KeyValue::new("stargate.outcome", outcome.to_string())]);
}

pub fn record_gateway_replay_bytes(bytes: usize) {
    METRICS.gateway_replay_bytes.record(bytes as u64, &[]);
}

pub fn record_gateway_policy(kind: &str, outcome: &str) {
    METRICS.gateway_policy_decisions.add(
        1,
        &[
            KeyValue::new("stargate.policy_kind", kind.to_string()),
            KeyValue::new("stargate.outcome", outcome.to_string()),
        ],
    );
}

pub fn record_propagation_draft(outcome: &'static str, kind: &'static str) {
    METRICS.gateway_propagation_drafts.add(
        1,
        &[
            KeyValue::new("stargate.outcome", outcome),
            KeyValue::new("stargate.kind", kind),
        ],
    );
}

#[cfg(all(feature = "postgres", feature = "redis"))]
pub fn record_audit_relay_batch(outcome: &str, records: usize, elapsed: Duration) {
    let attrs = [KeyValue::new("stargate.outcome", outcome.to_string())];
    METRICS.audit_relay_records.add(records as u64, &attrs);
    METRICS
        .audit_relay_duration
        .record(duration_ms(elapsed), &attrs);
    if outcome == "error" {
        METRICS.audit_relay_errors.add(1, &attrs);
    }
}

pub fn record_config_reload(kind: &str, outcome: &str) {
    METRICS.config_reloads.add(
        1,
        &[
            KeyValue::new("stargate.config_kind", kind.to_string()),
            KeyValue::new("stargate.outcome", outcome.to_string()),
        ],
    );
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod audit_trace_tests {
    use super::*;
    use opentelemetry::trace::TracerProvider as _;
    use tracing_subscriber::layer::SubscriberExt as _;

    #[test]
    fn audit_trace_id_accepts_only_valid_w3c_traceparent() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "traceparent",
            HeaderValue::from_static("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        );
        headers.insert(
            "x-trace-id",
            HeaderValue::from_static("client-selected-value"),
        );

        assert_eq!(
            trace_id_from_headers(&headers).as_deref(),
            Some("4bf92f3577b34da6a3ce929d0e0e4736")
        );

        headers.insert(
            "traceparent",
            HeaderValue::from_static("00-00000000000000000000000000000000-00f067aa0ba902b7-01"),
        );
        assert!(trace_id_from_headers(&headers).is_none());
    }

    #[test]
    fn server_span_has_a_trace_id_without_an_inbound_parent() {
        let provider = SdkTracerProvider::builder().build();
        let tracer = provider.tracer("stargate-test");
        let subscriber =
            tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("test_server_span", otel.kind = "server");
            assert!(trace_id_from_span(&span).is_some());
        });

        provider.shutdown().unwrap();
    }

    #[test]
    fn server_span_uses_a_valid_inbound_parent_trace_id() {
        global::set_text_map_propagator(TraceContextPropagator::new());
        let provider = SdkTracerProvider::builder().build();
        let tracer = provider.tracer("stargate-test");
        let subscriber =
            tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
        let mut headers = HeaderMap::new();
        headers.insert(
            "traceparent",
            HeaderValue::from_static("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        );

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("test_server_span", otel.kind = "server");
            set_span_parent_from_headers(&span, &headers);
            assert_eq!(
                trace_id_from_span(&span).as_deref(),
                Some("4bf92f3577b34da6a3ce929d0e0e4736")
            );
        });

        provider.shutdown().unwrap();
    }
}
