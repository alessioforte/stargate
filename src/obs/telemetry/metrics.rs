use super::INSTRUMENTATION_NAME;
use once_cell::sync::Lazy;
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram},
};
use std::time::Duration;

struct Metrics {
    http_server_requests: Counter<u64>,
    http_server_duration: Histogram<f64>,
    gateway_requests: Counter<u64>,
    gateway_duration: Histogram<f64>,
    gateway_transfer_duration: Histogram<f64>,
    gateway_upstream_requests: Counter<u64>,
    gateway_upstream_duration: Histogram<f64>,
    gateway_failovers: Counter<u64>,
    gateway_mirrors: Counter<u64>,
    gateway_response_disposals: Counter<u64>,
    gateway_response_disposal_bytes: Histogram<u64>,
    gateway_response_disposal_duration: Histogram<f64>,
    gateway_replay_bytes: Histogram<u64>,
    gateway_resources: opentelemetry::metrics::UpDownCounter<i64>,
    gateway_rejections: Counter<u64>,
    gateway_timeouts: Counter<u64>,
    gateway_policy_decisions: Counter<u64>,
    gateway_propagation_drafts: Counter<u64>,
    gateway_internal_context_issues: Counter<u64>,
    gateway_internal_context_signing_duration: Histogram<f64>,
    gateway_internal_context_queue_duration: Histogram<f64>,
    gateway_internal_context_token_size: Histogram<u64>,
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
            .with_description("Gateway request duration through response headers")
            .with_unit("ms")
            .build(),
        gateway_transfer_duration: meter
            .f64_histogram("stargate.gateway.transfer.duration")
            .with_description("Gateway upload, downstream body, or WebSocket lifetime")
            .with_unit("ms")
            .build(),
        gateway_upstream_requests: meter
            .u64_counter("stargate.gateway.upstream.requests")
            .with_description("Gateway upstream or direct-response attempts")
            .build(),
        gateway_upstream_duration: meter
            .f64_histogram("stargate.gateway.upstream.duration")
            .with_description(
                "Gateway upstream or direct-response attempt through response headers",
            )
            .with_unit("ms")
            .build(),
        gateway_failovers: meter
            .u64_counter("stargate.gateway.failovers")
            .with_description("Gateway failover attempts")
            .build(),
        gateway_mirrors: meter
            .u64_counter("stargate.gateway.mirrors")
            .with_description("Gateway mirror dispatches and completed outcomes")
            .build(),
        gateway_response_disposals: meter
            .u64_counter("stargate.gateway.response.disposals")
            .with_description("Discarded gateway response body outcomes")
            .build(),
        gateway_response_disposal_bytes: meter
            .u64_histogram("stargate.gateway.response.disposal.bytes")
            .with_description("Response data bytes observed during bounded disposal")
            .with_unit("By")
            .build(),
        gateway_response_disposal_duration: meter
            .f64_histogram("stargate.gateway.response.disposal.duration")
            .with_description("Discarded response body disposal duration")
            .with_unit("ms")
            .build(),
        gateway_resources: meter
            .i64_up_down_counter("stargate.gateway.resources.active")
            .with_description(
                "Active primary requests, mirror tasks, and reserved replay bytes (kind)",
            )
            .build(),
        gateway_rejections: meter.u64_counter("stargate.gateway.rejections").build(),
        gateway_timeouts: meter.u64_counter("stargate.gateway.timeouts").build(),
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
        gateway_internal_context_issues: meter
            .u64_counter("stargate.gateway.internal_context.issues")
            .with_description("Internal-context issuance outcomes")
            .build(),
        gateway_internal_context_signing_duration: meter
            .f64_histogram("stargate.gateway.internal_context.signing.duration")
            .with_description("Internal-context RS256 signing duration")
            .with_unit("ms")
            .build(),
        gateway_internal_context_queue_duration: meter
            .f64_histogram("stargate.gateway.internal_context.queue.duration")
            .with_description("Internal-context signing queue wait")
            .with_unit("ms")
            .build(),
        gateway_internal_context_token_size: meter
            .u64_histogram("stargate.gateway.internal_context.token.size")
            .with_description("Issued compact internal-context token size")
            .with_unit("By")
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

pub fn record_gateway_response_disposal(
    kind: &'static str,
    outcome: &'static str,
    bytes: usize,
    elapsed: Duration,
) {
    let attrs = [
        KeyValue::new("stargate.disposal.kind", kind),
        KeyValue::new("stargate.outcome", outcome),
    ];
    METRICS.gateway_response_disposals.add(1, &attrs);
    METRICS
        .gateway_response_disposal_bytes
        .record(bytes as u64, &attrs);
    METRICS
        .gateway_response_disposal_duration
        .record(duration_ms(elapsed), &attrs);
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

pub fn record_internal_context_queue(
    service: &str,
    dispatch_kind: &'static str,
    elapsed: Duration,
) {
    METRICS.gateway_internal_context_queue_duration.record(
        duration_ms(elapsed),
        &[
            KeyValue::new("stargate.service", service.to_owned()),
            KeyValue::new("stargate.dispatch.kind", dispatch_kind),
        ],
    );
}

pub fn record_internal_context_issue(
    service: &str,
    dispatch_kind: &'static str,
    outcome: &'static str,
    reason: &'static str,
    signing_elapsed: Option<Duration>,
    token_bytes: Option<usize>,
) {
    let attrs = [
        KeyValue::new("stargate.service", service.to_owned()),
        KeyValue::new("stargate.dispatch.kind", dispatch_kind),
        KeyValue::new("stargate.outcome", outcome),
        KeyValue::new("stargate.reason", reason),
    ];
    METRICS.gateway_internal_context_issues.add(1, &attrs);
    if let Some(elapsed) = signing_elapsed {
        METRICS
            .gateway_internal_context_signing_duration
            .record(duration_ms(elapsed), &attrs);
    }
    if let Some(bytes) = token_bytes {
        METRICS
            .gateway_internal_context_token_size
            .record(bytes as u64, &attrs);
    }
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

pub fn record_gateway_resource(kind: &'static str, delta: i64) {
    METRICS
        .gateway_resources
        .add(delta, &[KeyValue::new("kind", kind)]);
}
pub fn record_gateway_rejection(kind: &'static str) {
    METRICS
        .gateway_rejections
        .add(1, &[KeyValue::new("kind", kind)]);
}
pub fn record_gateway_timeout(phase: &'static str) {
    METRICS
        .gateway_timeouts
        .add(1, &[KeyValue::new("phase", phase)]);
}

pub fn record_gateway_transfer(phase: &'static str, outcome: &'static str, elapsed: Duration) {
    METRICS.gateway_transfer_duration.record(
        duration_ms(elapsed),
        &[
            KeyValue::new("phase", phase),
            KeyValue::new("stargate.outcome", outcome),
        ],
    );
}
