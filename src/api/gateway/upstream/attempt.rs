use super::SelectedService;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::observability::telemetry;
use axum::response::Response;
use ctx::DispatchKind;
use gate::DynLoadBalancer;
use std::{collections::HashMap, time::Instant};

/// Upstream status codes treated as upstream-health failures by the circuit
/// breaker. These are gateway/infrastructure errors (the upstream itself is
/// unreachable or overloaded) rather than application errors like 500, so they
/// must not be conflated with normal handler failures.
const UNHEALTHY_STATUSES: [u16; 3] = [502, 503, 504];

#[derive(Clone, Copy)]
pub(in crate::api::gateway) struct DispatchAttempt {
    pub(super) kind: DispatchKind,
    pub(super) number: u16,
}

impl DispatchAttempt {
    pub(super) const fn primary() -> Self {
        Self {
            kind: DispatchKind::Primary,
            number: 1,
        }
    }

    pub(super) const fn kind_label(self) -> &'static str {
        dispatch_kind_label(self.kind)
    }
}

/// Borrowed labels and timing for one network attempt. Recording is explicit:
/// preparation errors that return before a handler result are not observed.
pub(super) struct AttemptObservation<'a> {
    service: &'a str,
    base_url: &'a str,
    protocol: &'a str,
    dispatch: DispatchAttempt,
    pub(super) span: tracing::Span,
    started: Instant,
}

impl<'a> AttemptObservation<'a> {
    pub(super) fn start(
        service: &'a str,
        base_url: &'a str,
        protocol: &'a str,
        dispatch: DispatchAttempt,
        signed: bool,
    ) -> Self {
        let span = tracing::info_span!(
            "gateway.upstream",
            otel.kind = "client",
            stargate.service = %service,
            stargate.target_kind = "upstream",
            network.protocol.name = protocol,
            stargate.upstream_base_url = %base_url,
            stargate.dispatch.kind = tracing::field::Empty,
            stargate.dispatch.attempt = tracing::field::Empty,
            http.response.status_code = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
        );
        if signed {
            span.record("stargate.dispatch.kind", dispatch.kind_label());
            span.record("stargate.dispatch.attempt", u64::from(dispatch.number));
        }
        Self {
            service,
            base_url,
            protocol,
            dispatch,
            span,
            started: Instant::now(),
        }
    }

    pub(super) fn record_result(
        &self,
        balancers: &HashMap<String, DynLoadBalancer>,
        result: &Result<Response, ErrorResponse>,
    ) {
        record_attempt_span(&self.span, result);
        record_attempt_metrics(
            self.service,
            "upstream",
            self.protocol,
            result,
            self.started.elapsed(),
        );
        if self.dispatch.kind == DispatchKind::Primary {
            record_upstream_health(balancers, self.service, self.base_url, result);
        }
    }
}

pub(super) const fn dispatch_kind_label(kind: DispatchKind) -> &'static str {
    match kind {
        DispatchKind::Primary => "primary",
        DispatchKind::Shadow => "shadow",
    }
}

/// Feed a live upstream attempt back into its load balancer's circuit breaker:
/// a transport error or an infrastructure 5xx marks the upstream dead, any
/// other received response marks it alive.
fn record_upstream_health(
    balancers: &HashMap<String, DynLoadBalancer>,
    service_name: &str,
    base_url: &str,
    result: &Result<Response, ErrorResponse>,
) {
    let Some(lb) = balancers.get(service_name) else {
        return;
    };
    let healthy = match result {
        Err(error) if error.code == ErrorCode::UpstreamConnectionFailed => false,
        Err(_) => return,
        Ok(response) => !UNHEALTHY_STATUSES.contains(&response.status().as_u16()),
    };
    if healthy {
        lb.mark_alive(base_url);
    } else {
        lb.mark_dead(base_url);
    }
}

pub(super) fn attempt_limit_failure(service: &str, dispatch_kind: &'static str) -> ErrorResponse {
    telemetry::record_internal_context_issue(
        service,
        dispatch_kind,
        "failure",
        "attempt",
        None,
        None,
    );
    tracing::warn!(
        stargate.service = service,
        stargate.dispatch.kind = dispatch_kind,
        stargate.outcome = "failure",
        stargate.reason = "attempt",
        "Internal-context dispatch attempt limit exceeded"
    );
    ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed)
}

pub(in crate::api::gateway) fn next_dispatch_attempt(
    selected: &SelectedService,
    kind: DispatchKind,
    network_attempt: &mut u16,
) -> Result<Option<DispatchAttempt>, ErrorResponse> {
    let SelectedService::Upstream { service_name, .. } = selected else {
        return Ok(None);
    };
    *network_attempt = (*network_attempt)
        .checked_add(1)
        .ok_or_else(|| attempt_limit_failure(service_name, dispatch_kind_label(kind)))?;
    Ok(Some(DispatchAttempt {
        kind,
        number: *network_attempt,
    }))
}

fn result_status(result: &Result<Response, ErrorResponse>) -> u16 {
    match result {
        Ok(response) => response.status().as_u16(),
        Err(error) => error.status.as_u16(),
    }
}

fn result_outcome(result: &Result<Response, ErrorResponse>) -> &'static str {
    match result {
        Ok(_) => "success",
        Err(_) => "error",
    }
}

fn record_attempt_span(span: &tracing::Span, result: &Result<Response, ErrorResponse>) {
    let status = result_status(result);
    span.record("http.response.status_code", status);
    if status >= 500 {
        span.record("otel.status_code", "ERROR");
    }
}

pub(super) fn record_attempt_metrics(
    service: &str,
    target_kind: &str,
    protocol: &str,
    result: &Result<Response, ErrorResponse>,
    elapsed: std::time::Duration,
) {
    telemetry::record_gateway_upstream_attempt(
        service,
        target_kind,
        protocol,
        Some(result_status(result)),
        result_outcome(result),
        elapsed,
    );
}

#[cfg(test)]
mod tests;
