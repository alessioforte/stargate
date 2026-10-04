use super::{
    dispatch::InternalDispatch,
    http,
    responses::build_direct_response,
    types::{DynLoadBalancer, ReplayRequest, RequestState, SelectedService},
    ws,
};
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::{ext::RequestExt, gate::RuntimeSnapshot, telemetry};
#[cfg(test)]
use ::http::HeaderMap;
use ::http::Request;
use axum::{body::Body, response::Response};
use ctx::DispatchKind;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tracing::Instrument;

/// Upstream status codes treated as upstream-health failures by the circuit
/// breaker. These are gateway/infrastructure errors (the upstream itself is
/// unreachable or overloaded) rather than application errors like 500, so they
/// must not be conflated with normal handler failures.
const UNHEALTHY_STATUSES: [u16; 3] = [502, 503, 504];

#[derive(Clone, Copy)]
pub(super) struct DispatchAttempt {
    kind: DispatchKind,
    number: u16,
}

impl DispatchAttempt {
    const fn primary() -> Self {
        Self {
            kind: DispatchKind::Primary,
            number: 1,
        }
    }

    const fn kind_label(self) -> &'static str {
        dispatch_kind_label(self.kind)
    }
}

const fn dispatch_kind_label(kind: DispatchKind) -> &'static str {
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

pub(super) async fn execute_selected_with_request(
    runtime: &Arc<RuntimeSnapshot>,
    selected: &SelectedService,
    req: Request<Body>,
    state: &RequestState,
) -> Result<Response, ErrorResponse> {
    state.execution.check()?;
    match selected {
        SelectedService::DirectResponse {
            status,
            headers,
            body,
        } => {
            let started = Instant::now();
            let result = build_direct_response(*status, headers, body.as_ref());
            record_attempt_metrics(
                "direct_response",
                "direct_response",
                "http",
                &result,
                started.elapsed(),
            );
            result
        }
        SelectedService::Upstream {
            service_name,
            upstream_base_url,
            internal_context,
        } => {
            let attempt = DispatchAttempt::primary();
            let protocol = req.get_protocol().to_string();
            let uri = upstream_uri(upstream_base_url, state);
            let span = tracing::info_span!(
                "gateway.upstream",
                otel.kind = "client",
                stargate.service = %service_name,
                stargate.target_kind = "upstream",
                network.protocol.name = %protocol,
                stargate.upstream_base_url = %upstream_base_url,
                stargate.dispatch.kind = tracing::field::Empty,
                stargate.dispatch.attempt = tracing::field::Empty,
                http.response.status_code = tracing::field::Empty,
                otel.status_code = tracing::field::Empty,
            );
            if internal_context.is_some() {
                span.record("stargate.dispatch.kind", attempt.kind_label());
                span.record("stargate.dispatch.attempt", u64::from(attempt.number));
            }
            let started = Instant::now();
            let internal_dispatch = internal_context
                .as_ref()
                .map(|settings| {
                    InternalDispatch::new(
                        service_name,
                        &settings.audience,
                        state.propagation_draft.as_ref(),
                        state.internal_context_runtime.as_ref(),
                        attempt.kind,
                        attempt.number,
                    )
                })
                .transpose()?;

            if protocol == "ws" {
                let uri = websocket_uri(&uri);
                let result = ws::handler(
                    runtime.clone(),
                    req,
                    &uri,
                    state.preserve_host,
                    internal_dispatch.as_ref(),
                    &state.execution,
                )
                .instrument(span.clone())
                .await;
                record_attempt_span(&span, &result);
                record_attempt_metrics(
                    service_name,
                    "upstream",
                    &protocol,
                    &result,
                    started.elapsed(),
                );
                record_upstream_health(
                    &runtime.core.balancers,
                    service_name,
                    upstream_base_url,
                    &result,
                );
                return result;
            }

            let client = runtime
                .client(service_name)
                .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayHttpClientUnavailable))?;
            let result = http::handler(
                req,
                &uri,
                &client,
                state.preserve_host,
                internal_dispatch.as_ref(),
                &runtime.settings,
                &state.execution,
            )
            .instrument(span.clone())
            .await;
            record_attempt_span(&span, &result);
            record_attempt_metrics(
                service_name,
                "upstream",
                &protocol,
                &result,
                started.elapsed(),
            );
            record_upstream_health(
                &runtime.core.balancers,
                service_name,
                upstream_base_url,
                &result,
            );
            result
        }
    }
}

pub(super) async fn execute_selected_from_replay(
    runtime: &Arc<RuntimeSnapshot>,
    selected: &SelectedService,
    replay: &ReplayRequest,
    state: &RequestState,
    dispatch_attempt: Option<DispatchAttempt>,
) -> Result<Response, ErrorResponse> {
    state.execution.check()?;
    match selected {
        SelectedService::DirectResponse {
            status,
            headers,
            body,
        } => {
            let started = Instant::now();
            let result = build_direct_response(*status, headers, body.as_ref());
            record_attempt_metrics(
                "direct_response",
                "direct_response",
                "http",
                &result,
                started.elapsed(),
            );
            result
        }
        SelectedService::Upstream {
            service_name,
            upstream_base_url,
            internal_context,
        } => {
            let dispatch_attempt =
                dispatch_attempt.ok_or_else(|| attempt_limit_failure(service_name, "unknown"))?;
            let uri = upstream_uri(upstream_base_url, state);
            let req = replay.build(&uri)?;
            let internal_dispatch = internal_context
                .as_ref()
                .map(|settings| {
                    InternalDispatch::new(
                        service_name,
                        &settings.audience,
                        state.propagation_draft.as_ref(),
                        state.internal_context_runtime.as_ref(),
                        dispatch_attempt.kind,
                        dispatch_attempt.number,
                    )
                })
                .transpose()?;
            let client = runtime
                .client(service_name)
                .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayHttpClientUnavailable))?;

            let span = tracing::info_span!(
                "gateway.upstream",
                otel.kind = "client",
                stargate.service = %service_name,
                stargate.target_kind = "upstream",
                network.protocol.name = "http",
                stargate.upstream_base_url = %upstream_base_url,
                stargate.dispatch.kind = tracing::field::Empty,
                stargate.dispatch.attempt = tracing::field::Empty,
                http.response.status_code = tracing::field::Empty,
                otel.status_code = tracing::field::Empty,
            );
            if internal_context.is_some() {
                span.record("stargate.dispatch.kind", dispatch_attempt.kind_label());
                span.record(
                    "stargate.dispatch.attempt",
                    u64::from(dispatch_attempt.number),
                );
            }
            let started = Instant::now();
            let result = http::handler(
                req,
                &uri,
                &client,
                state.preserve_host,
                internal_dispatch.as_ref(),
                &runtime.settings,
                &state.execution,
            )
            .instrument(span.clone())
            .await;
            record_attempt_span(&span, &result);
            record_attempt_metrics(service_name, "upstream", "http", &result, started.elapsed());
            if dispatch_attempt.kind == DispatchKind::Primary {
                record_upstream_health(
                    &runtime.core.balancers,
                    service_name,
                    upstream_base_url,
                    &result,
                );
            }
            result
        }
    }
}

fn upstream_uri(base_url: &str, state: &RequestState) -> String {
    let mut uri = format!("{}{}", base_url, state.path);
    if !state.query.is_empty() {
        uri.push('?');
        uri.push_str(&state.query);
    }
    uri
}

fn websocket_uri(uri: &str) -> String {
    if let Some(rest) = uri.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = uri.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        uri.to_owned()
    }
}

fn attempt_limit_failure(service: &str, dispatch_kind: &'static str) -> ErrorResponse {
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

pub(super) fn next_dispatch_attempt(
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

fn result_status(result: &Result<Response, ErrorResponse>) -> Option<u16> {
    match result {
        Ok(response) => Some(response.status().as_u16()),
        Err(error) => Some(error.status.as_u16()),
    }
}

fn result_outcome(result: &Result<Response, ErrorResponse>) -> &'static str {
    match result {
        Ok(_) => "success",
        Err(_) => "error",
    }
}

fn record_attempt_span(span: &tracing::Span, result: &Result<Response, ErrorResponse>) {
    let Some(status) = result_status(result) else {
        span.record("otel.status_code", "ERROR");
        return;
    };
    span.record("http.response.status_code", status);
    if status >= 500 {
        span.record("otel.status_code", "ERROR");
    }
}

fn record_attempt_metrics(
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
        result_status(result),
        result_outcome(result),
        elapsed,
    );
}

#[cfg(test)]
#[path = "executor_disposal_tests.rs"]
mod disposal_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::gateway::{execution::execute_plan_from_replay, types::ExecutionPlan};
    use crate::etc::reqctx::INTERNAL_CONTEXT_HEADER;
    use gate::graph::{HeaderValueNode, InternalContextNode};
    use std::collections::HashMap;

    // Single-upstream balancer with the given open-after threshold and a long
    // cooldown, so a tripped breaker stays open for the test.
    pub(super) fn single_upstream(
        service: &str,
        url: &str,
        threshold: usize,
    ) -> HashMap<String, DynLoadBalancer> {
        let upstream = lb::Upstream::with_circuit_breaker(url.to_string(), None, threshold, 3600);
        let balancer: DynLoadBalancer =
            lb::BaseLoadBalancer::new(lb::RoundRobin::new(), vec![upstream]);
        let mut map = HashMap::new();
        map.insert(service.to_string(), balancer);
        map
    }

    fn runtime() -> Arc<RuntimeSnapshot> {
        let config = gate::cfg::RuntimeConfig::from_yaml_str(
            r#"
schema: stargate/v1
ingress:
  limit: ingress
  timeout: 250ms
limits:
  ingress:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 100ms
http:
  upstreams:
    test:
      targets:
        - url: http://127.0.0.1:1
  services:
    unreachable:
      kind: load_balancer
      upstream: test
    orders:
      kind: load_balancer
      upstream: test
"#,
        )
        .unwrap();
        crate::etc::gate::test_support::runtime(config)
    }

    fn ctx() -> lb::RequestContext<'static> {
        lb::RequestContext {
            client_ip: "127.0.0.1",
            path: "/",
            method: "GET",
            key: None,
        }
    }

    fn response(status: u16) -> Result<Response, ErrorResponse> {
        Ok(Response::builder()
            .status(status)
            .body(Body::empty())
            .unwrap())
    }

    fn transport_error() -> Result<Response, ErrorResponse> {
        Err(ErrorResponse::new(ErrorCode::UpstreamConnectionFailed))
    }

    pub(super) fn available(balancers: &HashMap<String, DynLoadBalancer>, service: &str) -> bool {
        balancers.get(service).unwrap().select(&ctx()).is_some()
    }

    pub(super) fn replay_state() -> RequestState {
        RequestState {
            execution: crate::api::gateway::lifecycle::Execution::default(),
            client_ip: "127.0.0.1".into(),
            load_balancer_key: None,
            original_path: "/orders".to_owned(),
            path: "/orders".to_owned(),
            query: String::new(),
            preserve_host: false,
            response_headers: Default::default(),
            propagation_draft: None,
            internal_context_runtime: None,
        }
    }

    pub(super) fn replay_request() -> ReplayRequest {
        ReplayRequest {
            method: ::http::Method::GET,
            version: ::http::Version::HTTP_11,
            headers: HeaderMap::new(),
            body: hyper::body::Bytes::new(),
        }
    }

    #[test]
    fn websocket_uri_maps_http_schemes_and_preserves_the_rest() {
        assert_eq!(
            websocket_uri("https://upstream.example/socket?token=abc"),
            "wss://upstream.example/socket?token=abc"
        );
        assert_eq!(
            websocket_uri("ws://upstream.example/socket"),
            "ws://upstream.example/socket"
        );
        assert_eq!(
            websocket_uri("wss://upstream.example/socket"),
            "wss://upstream.example/socket"
        );
    }

    #[tokio::test]
    async fn direct_response_remains_local_and_has_no_internal_context() {
        let selected = SelectedService::DirectResponse {
            status: 202,
            headers: vec![HeaderValueNode {
                name: "x-direct".to_owned(),
                value: "yes".to_owned(),
            }],
            body: None,
        };
        let state = RequestState {
            execution: crate::api::gateway::lifecycle::Execution::default(),
            client_ip: "127.0.0.1".into(),
            load_balancer_key: None,
            original_path: "/direct".to_owned(),
            path: "/direct".to_owned(),
            query: String::new(),
            preserve_host: false,
            response_headers: Default::default(),
            propagation_draft: None,
            internal_context_runtime: None,
        };
        let request = Request::builder().body(Body::empty()).unwrap();

        let response = execute_selected_with_request(&runtime(), &selected, request, &state)
            .await
            .unwrap();

        assert_eq!(response.status(), 202);
        assert_eq!(response.headers()["x-direct"], "yes");
        assert!(response.headers().get(INTERNAL_CONTEXT_HEADER).is_none());
    }

    #[tokio::test]
    async fn status_failover_advances_to_the_next_plan_entry() {
        let plan = ExecutionPlan::Failover {
            services: vec![
                ExecutionPlan::DirectResponse {
                    status: 503,
                    headers: Vec::new(),
                    body: None,
                },
                ExecutionPlan::DirectResponse {
                    status: 204,
                    headers: Vec::new(),
                    body: None,
                },
            ],
            on_status: vec![503],
        };

        let response = execute_plan_from_replay(
            &runtime(),
            &plan,
            &replay_request(),
            &replay_state(),
            DispatchKind::Primary,
        )
        .await
        .unwrap();

        assert_eq!(response.status(), 204);
    }

    #[tokio::test]
    async fn transport_failure_advances_even_without_status_rules() {
        let plan = ExecutionPlan::Failover {
            services: vec![
                ExecutionPlan::Upstream {
                    service_name: "unreachable".to_owned(),
                    internal_context: None,
                },
                ExecutionPlan::DirectResponse {
                    status: 204,
                    headers: Vec::new(),
                    body: None,
                },
            ],
            on_status: Vec::new(),
        };

        let response = execute_plan_from_replay(
            &runtime(),
            &plan,
            &replay_request(),
            &replay_state(),
            DispatchKind::Primary,
        )
        .await
        .unwrap();

        assert_eq!(response.status(), 204);
    }

    #[tokio::test]
    async fn internal_context_failure_never_fails_over() {
        let plan = ExecutionPlan::Failover {
            services: vec![
                ExecutionPlan::Upstream {
                    service_name: "orders".to_owned(),
                    internal_context: Some(InternalContextNode {
                        audience: "urn:stargate:service:orders".to_owned(),
                    }),
                },
                ExecutionPlan::DirectResponse {
                    status: 204,
                    headers: Vec::new(),
                    body: None,
                },
            ],
            on_status: Vec::new(),
        };

        let error = execute_plan_from_replay(
            &runtime(),
            &plan,
            &replay_request(),
            &replay_state(),
            DispatchKind::Primary,
        )
        .await
        .unwrap_err();

        assert_eq!(error.status, ::http::StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn dispatch_attempts_count_only_network_calls_and_reset_by_kind() {
        let direct = SelectedService::DirectResponse {
            status: 200,
            headers: Vec::new(),
            body: None,
        };
        let upstream = SelectedService::Upstream {
            service_name: "orders".to_owned(),
            upstream_base_url: "http://orders.test".to_owned(),
            internal_context: None,
        };
        let mut primary_counter = 0;
        let mut shadow_counter = 0;

        assert!(
            next_dispatch_attempt(&direct, DispatchKind::Primary, &mut primary_counter)
                .unwrap()
                .is_none()
        );
        let primary_one =
            next_dispatch_attempt(&upstream, DispatchKind::Primary, &mut primary_counter)
                .unwrap()
                .unwrap();
        let primary_two =
            next_dispatch_attempt(&upstream, DispatchKind::Primary, &mut primary_counter)
                .unwrap()
                .unwrap();
        let shadow_one =
            next_dispatch_attempt(&upstream, DispatchKind::Shadow, &mut shadow_counter)
                .unwrap()
                .unwrap();

        assert_eq!(primary_one.number, 1);
        assert_eq!(primary_two.number, 2);
        assert_eq!(shadow_one.number, 1);
        assert_eq!(primary_one.kind, DispatchKind::Primary);
        assert_eq!(shadow_one.kind, DispatchKind::Shadow);
    }

    #[test]
    fn transport_error_then_success_toggles_availability() {
        let balancers = single_upstream("svc", "http://up", 1);
        record_upstream_health(&balancers, "svc", "http://up", &transport_error());
        assert!(
            !available(&balancers, "svc"),
            "transport error should eject"
        );
        record_upstream_health(&balancers, "svc", "http://up", &response(200));
        assert!(available(&balancers, "svc"), "success should restore");
    }

    #[test]
    fn infrastructure_5xx_ejects_but_app_5xx_does_not() {
        let balancers = single_upstream("svc", "http://up", 1);
        record_upstream_health(&balancers, "svc", "http://up", &response(503));
        assert!(!available(&balancers, "svc"), "503 is an upstream failure");

        let balancers = single_upstream("svc", "http://up", 1);
        record_upstream_health(&balancers, "svc", "http://up", &response(500));
        assert!(
            available(&balancers, "svc"),
            "500 is an application error, breaker stays closed"
        );
    }

    #[test]
    fn unknown_service_is_a_noop() {
        let balancers = single_upstream("svc", "http://up", 1);
        record_upstream_health(&balancers, "missing", "http://up", &transport_error());
        assert!(available(&balancers, "svc"));
    }
}
