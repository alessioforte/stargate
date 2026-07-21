use super::{
    http,
    responses::build_direct_response,
    types::{DynLoadBalancer, ExecutionPlan, ReplayRequest, RequestState, SelectedService},
    ws,
};
use crate::err::{ErrorResponse, HttpError};
use crate::etc::{ext::RequestExt, gate::get_client, telemetry};
use ::http::{HeaderMap, Request};
use axum::{body::Body, response::Response};
use http_body_util::BodyExt;
use std::{collections::HashMap, time::Instant};
use tracing::Instrument;

/// Upstream status codes treated as upstream-health failures by the circuit
/// breaker. These are gateway/infrastructure errors (the upstream itself is
/// unreachable or overloaded) rather than application errors like 500, so they
/// must not be conflated with normal handler failures.
const UNHEALTHY_STATUSES: [u16; 3] = [502, 503, 504];

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
        Err(_) => false,
        Ok(response) => !UNHEALTHY_STATUSES.contains(&response.status().as_u16()),
    };
    if healthy {
        lb.mark_alive(base_url);
    } else {
        lb.mark_dead(base_url);
    }
}

pub(super) async fn execute_selected_with_request(
    selected: &SelectedService,
    req: Request<Body>,
    state: &RequestState,
    balancers: &HashMap<String, DynLoadBalancer>,
) -> Result<Response, ErrorResponse> {
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
                span.record("stargate.dispatch.kind", "primary");
                span.record("stargate.dispatch.attempt", 1_u64);
            }
            let started = Instant::now();

            if protocol == "ws" {
                if internal_context.is_some() {
                    return Err(unsupported_internal_path("websocket"));
                }
                let result = ws::handler(req, &uri, state.preserve_host)
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
                record_upstream_health(balancers, service_name, upstream_base_url, &result);
                return result;
            }

            let client = get_client(service_name).ok_or_else(|| {
                ErrorResponse::from(HttpError::InternalServerError(
                    "HTTP client not found".to_string(),
                ))
            })?;
            let internal_dispatch = internal_context
                .as_ref()
                .map(|settings| {
                    http::InternalDispatch::primary(
                        &settings.audience,
                        state.propagation_draft.as_ref(),
                    )
                })
                .transpose()?;

            let empty_headers = HeaderMap::new();
            let result = http::handler(
                req,
                &empty_headers,
                &uri,
                &client,
                state.preserve_host,
                internal_dispatch.as_ref(),
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
            record_upstream_health(balancers, service_name, upstream_base_url, &result);
            result
        }
    }
}

async fn execute_selected_from_replay(
    selected: &SelectedService,
    replay: &ReplayRequest,
    state: &RequestState,
    balancers: Option<&HashMap<String, DynLoadBalancer>>,
) -> Result<Response, ErrorResponse> {
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
            if internal_context.is_some() {
                return Err(unsupported_internal_path("replay"));
            }
            let uri = upstream_uri(upstream_base_url, state);
            let req = replay.build(&uri)?;
            let client = get_client(service_name).ok_or_else(|| {
                ErrorResponse::from(HttpError::InternalServerError(
                    "HTTP client not found".to_string(),
                ))
            })?;

            let span = tracing::info_span!(
                "gateway.upstream",
                otel.kind = "client",
                stargate.service = %service_name,
                stargate.target_kind = "upstream",
                network.protocol.name = "http",
                stargate.upstream_base_url = %upstream_base_url,
                http.response.status_code = tracing::field::Empty,
                otel.status_code = tracing::field::Empty,
            );
            let started = Instant::now();
            let empty_headers = HeaderMap::new();
            let result = http::handler(
                req,
                &empty_headers,
                &uri,
                &client,
                state.preserve_host,
                None,
            )
            .instrument(span.clone())
            .await;
            record_attempt_span(&span, &result);
            record_attempt_metrics(service_name, "upstream", "http", &result, started.elapsed());
            if let Some(balancers) = balancers {
                record_upstream_health(balancers, service_name, upstream_base_url, &result);
            }
            result
        }
    }
}

pub(super) async fn execute_plan_from_replay(
    plan: &ExecutionPlan,
    replay: &ReplayRequest,
    state: &RequestState,
    balancers: Option<&HashMap<String, DynLoadBalancer>>,
) -> Result<Response, ErrorResponse> {
    let last_idx = plan.attempts.len().saturating_sub(1);
    for (idx, selected) in plan.attempts.iter().enumerate() {
        let response = execute_selected_from_replay(selected, replay, state, balancers)
            .instrument(tracing::debug_span!(
                "gateway.replay_attempt",
                attempt = idx,
                stargate.service = %selected_service_name(selected),
                stargate.target_kind = selected_target_kind(selected),
            ))
            .await?;
        if idx < last_idx && plan.should_failover_response(response.status()) {
            let status = response.status();
            telemetry::record_gateway_failover("status", Some(status.as_u16()));
            tracing::warn!(
                status = status.as_u16(),
                attempt = idx,
                "Failing over response by status"
            );
            if let Err(error) = response.into_body().collect().await {
                tracing::warn!(%status, %error, "Failover response drain failed");
            }
            continue;
        }
        return Ok(response);
    }

    Err(ErrorResponse::from(HttpError::ServiceUnavailable(
        "No healthy upstream available".to_string(),
    )))
}

pub(super) fn spawn_mirrors(
    mirrors: Vec<ExecutionPlan>,
    replay: ReplayRequest,
    state: RequestState,
) {
    for mirror in mirrors {
        telemetry::record_gateway_mirror("dispatched");
        let replay = replay.clone();
        let state = state.clone();
        let span = tracing::debug_span!("gateway.mirror");
        tokio::spawn(
            async move {
                // Mirror traffic is shadow traffic: its outcomes must not feed the
                // circuit breaker, so pass no balancers.
                match execute_plan_from_replay(&mirror, &replay, &state, None).await {
                    Ok(response) => {
                        telemetry::record_gateway_mirror("success");
                        let status = response.status();
                        if let Err(error) = response.into_body().collect().await {
                            tracing::warn!(%status, %error, "Mirror response drain failed");
                        }
                    }
                    Err(error) => {
                        telemetry::record_gateway_mirror("error");
                        tracing::warn!(%error, "Mirror request failed");
                    }
                }
            }
            .instrument(span),
        );
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

fn unsupported_internal_path(path: &'static str) -> ErrorResponse {
    telemetry::record_internal_context_issue("failure", "execution_path");
    tracing::warn!(
        path,
        "Internal-context execution path remains activation guarded"
    );
    ErrorResponse::from(HttpError::InternalServerError(
        "Internal upstream request preparation failed".to_string(),
    ))
}

fn selected_service_name(selected: &SelectedService) -> &str {
    match selected {
        SelectedService::Upstream { service_name, .. } => service_name,
        SelectedService::DirectResponse { .. } => "direct_response",
    }
}

fn selected_target_kind(selected: &SelectedService) -> &str {
    match selected {
        SelectedService::Upstream { .. } => "upstream",
        SelectedService::DirectResponse { .. } => "direct_response",
    }
}

fn result_status(result: &Result<Response, ErrorResponse>) -> Option<u16> {
    match result {
        Ok(response) => Some(response.status().as_u16()),
        Err(error) => Some(error.code.as_u16()),
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
mod tests {
    use super::*;
    use crate::etc::reqctx::INTERNAL_CONTEXT_HEADER;
    use gate::graph::HeaderValueNode;
    use std::collections::HashMap;

    // Single-upstream balancer with the given open-after threshold and a long
    // cooldown, so a tripped breaker stays open for the test.
    fn single_upstream(
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
        Err(ErrorResponse::from(HttpError::BadGateway(
            "boom".to_string(),
        )))
    }

    fn available(balancers: &HashMap<String, DynLoadBalancer>, service: &str) -> bool {
        balancers.get(service).unwrap().select(&ctx()).is_some()
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
            original_path: "/direct".to_owned(),
            path: "/direct".to_owned(),
            query: String::new(),
            preserve_host: false,
            response_headers: Default::default(),
            propagation_draft: None,
        };
        let request = Request::builder().body(Body::empty()).unwrap();

        let response = execute_selected_with_request(&selected, request, &state, &HashMap::new())
            .await
            .unwrap();

        assert_eq!(response.status(), 202);
        assert_eq!(response.headers()["x-direct"], "yes");
        assert!(response.headers().get(INTERNAL_CONTEXT_HEADER).is_none());
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
