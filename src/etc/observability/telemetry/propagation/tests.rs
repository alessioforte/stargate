use super::*;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
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
fn trace_only_injection_never_leaks_raw_headers_without_an_active_sdk_span() {
    let dispatch = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    tracing::dispatcher::with_default(&dispatch, || {
        let mut headers = HeaderMap::new();
        headers.insert(
            "traceparent",
            HeaderValue::from_static("00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01"),
        );
        headers.insert("tracestate", HeaderValue::from_static("attacker=value"));

        inject_trace_context(&mut headers);

        assert!(headers.get("traceparent").is_none());
        assert!(headers.get("tracestate").is_none());
    });
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
