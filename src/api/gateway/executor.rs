use super::{
    http,
    responses::build_direct_response,
    types::{ExecutionPlan, ReplayRequest, RequestState, SelectedService},
    ws,
};
use crate::err::{ErrorResponse, HttpError};
use crate::etc::{ext::RequestExt, gate::get_client};
use ::http::{HeaderMap, Request};
use axum::{body::Body, response::Response};
use http_body_util::BodyExt;

pub(super) async fn execute_selected_with_request(
    selected: &SelectedService,
    req: Request<Body>,
    state: &RequestState,
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
                return ws::handler(req, &uri, state.preserve_host).await;
            }

            let client = get_client(service_name).ok_or_else(|| {
                ErrorResponse::from(HttpError::InternalServerError(
                    "HTTP client not found".to_string(),
                ))
            })?;

            let empty_headers = HeaderMap::new();
            http::handler(req, &empty_headers, &uri, &client, state.preserve_host).await
        }
    }
}

async fn execute_selected_from_replay(
    selected: &SelectedService,
    replay: &ReplayRequest,
    state: &RequestState,
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
            http::handler(req, &empty_headers, &uri, &client, state.preserve_host).await
        }
    }
}

pub(super) async fn execute_plan_from_replay(
    plan: &ExecutionPlan,
    replay: &ReplayRequest,
    state: &RequestState,
) -> Result<Response, ErrorResponse> {
    let last_idx = plan.attempts.len().saturating_sub(1);
    for (idx, selected) in plan.attempts.iter().enumerate() {
        let response = execute_selected_from_replay(selected, replay, state).await?;
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
            match execute_plan_from_replay(&mirror, &replay, &state).await {
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
