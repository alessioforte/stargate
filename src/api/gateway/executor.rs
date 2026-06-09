use super::{
    http,
    responses::build_direct_response,
    types::{DynLoadBalancer, ExecutionPlan, ReplayRequest, RequestState, SelectedService},
    ws,
};
use crate::err::{ErrorResponse, HttpError};
use crate::etc::{ext::RequestExt, gate::get_client};
use ::http::{HeaderMap, Request};
use axum::{body::Body, response::Response};
use http_body_util::BodyExt;
use std::collections::HashMap;

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
        } => build_direct_response(*status, headers, body.as_ref()),
        SelectedService::Upstream {
            service_name,
            upstream_base_url,
        } => {
            let uri = upstream_uri(upstream_base_url, state);
            if req.get_protocol() == "ws" {
                let result = ws::handler(req, &uri, state.preserve_host).await;
                record_upstream_health(balancers, service_name, upstream_base_url, &result);
                return result;
            }

            let client = get_client(service_name).ok_or_else(|| {
                ErrorResponse::from(HttpError::InternalServerError(
                    "HTTP client not found".to_string(),
                ))
            })?;

            let empty_headers = HeaderMap::new();
            let result =
                http::handler(req, &empty_headers, &uri, &client, state.preserve_host).await;
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
        } => build_direct_response(*status, headers, body.as_ref()),
        SelectedService::Upstream {
            service_name,
            upstream_base_url,
        } => {
            let uri = upstream_uri(upstream_base_url, state);
            let req = replay.build(&uri)?;
            let client = get_client(service_name).ok_or_else(|| {
                ErrorResponse::from(HttpError::InternalServerError(
                    "HTTP client not found".to_string(),
                ))
            })?;

            let empty_headers = HeaderMap::new();
            let result =
                http::handler(req, &empty_headers, &uri, &client, state.preserve_host).await;
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
        let response = execute_selected_from_replay(selected, replay, state, balancers).await?;
        if idx < last_idx && plan.should_failover_response(response.status()) {
            let status = response.status();
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
        let replay = replay.clone();
        let state = state.clone();
        tokio::spawn(async move {
            // Mirror traffic is shadow traffic: its outcomes must not feed the
            // circuit breaker, so pass no balancers.
            match execute_plan_from_replay(&mirror, &replay, &state, None).await {
                Ok(response) => {
                    let status = response.status();
                    if let Err(error) = response.into_body().collect().await {
                        tracing::warn!(%status, %error, "Mirror response drain failed");
                    }
                }
                Err(error) => tracing::warn!(%error, "Mirror request failed"),
            }
        });
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

#[cfg(test)]
mod tests {
    use super::*;
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
        Err(ErrorResponse::from(HttpError::BadGateway("boom".to_string())))
    }

    fn available(balancers: &HashMap<String, DynLoadBalancer>, service: &str) -> bool {
        balancers.get(service).unwrap().select(&ctx()).is_some()
    }

    #[test]
    fn transport_error_then_success_toggles_availability() {
        let balancers = single_upstream("svc", "http://up", 1);
        record_upstream_health(&balancers, "svc", "http://up", &transport_error());
        assert!(!available(&balancers, "svc"), "transport error should eject");
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
