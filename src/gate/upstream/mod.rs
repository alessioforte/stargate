pub(in crate::gate) mod attempt;
pub(in crate::gate) mod headers;
pub(in crate::gate) mod http;
pub(in crate::gate) mod internal_context;
pub(in crate::gate) mod uri;
pub(in crate::gate) mod websocket;
use attempt::{AttemptObservation, DispatchAttempt, attempt_limit_failure, record_attempt_metrics};
use gate::graph::{HeaderValueNode, InternalContextNode, ResponseBodyNode};

use crate::err::{ErrorCode, ErrorResponse};
use crate::gate::{
    execution::replay::ReplayRequest, request::RequestState, response::build_direct_response,
};
use crate::{gate::RuntimeSnapshot, web::request::RequestExt};
use ::http::Request;
use axum::{body::Body, response::Response};
use internal_context::InternalDispatch;
use std::{sync::Arc, time::Instant};
use tracing::Instrument;

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
        } => execute_direct_response(*status, headers, body.as_ref()),
        SelectedService::Upstream {
            service_name,
            upstream_base_url,
            internal_context,
            forward_credentials,
        } => {
            let mut upstream_headers = state.upstream_headers.clone();
            upstream_headers.forward_credentials = *forward_credentials;
            let attempt = DispatchAttempt::primary();
            let protocol = req.get_protocol();
            let uri = upstream_uri(upstream_base_url, state);
            // Live timing includes dispatch preparation and transport lookup.
            let observation = AttemptObservation::start(
                service_name,
                upstream_base_url,
                protocol,
                attempt,
                internal_context.is_some(),
            );
            let internal_dispatch =
                prepare_internal_dispatch(service_name, internal_context.as_ref(), state, attempt)?;

            let result = if protocol == "ws" {
                let transport = runtime
                    .transport(service_name)
                    .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayHttpClientUnavailable))?;
                let uri = websocket_uri(&uri);
                websocket::handler(
                    runtime.clone(),
                    transport,
                    req,
                    &uri,
                    &upstream_headers,
                    internal_dispatch.as_ref(),
                    &state.execution,
                )
                .instrument(observation.span.clone())
                .await
            } else {
                let client = runtime
                    .client(service_name)
                    .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayHttpClientUnavailable))?;
                http::handler(
                    req,
                    &uri,
                    &client,
                    &upstream_headers,
                    internal_dispatch.as_ref(),
                    &runtime.settings,
                    &state.execution,
                )
                .instrument(observation.span.clone())
                .await
            };
            observation.record_result(&runtime.core.balancers, &result);
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
        } => execute_direct_response(*status, headers, body.as_ref()),
        SelectedService::Upstream {
            service_name,
            upstream_base_url,
            internal_context,
            forward_credentials,
        } => {
            let mut upstream_headers = state.upstream_headers.clone();
            upstream_headers.forward_credentials = *forward_credentials;
            let attempt =
                dispatch_attempt.ok_or_else(|| attempt_limit_failure(service_name, "unknown"))?;
            let uri = upstream_uri(upstream_base_url, state);
            let req = replay.build(&uri)?;
            let internal_dispatch =
                prepare_internal_dispatch(service_name, internal_context.as_ref(), state, attempt)?;
            let client = runtime
                .client(service_name)
                .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayHttpClientUnavailable))?;

            // Replay always uses HTTP; timing starts after local preparation.
            let observation = AttemptObservation::start(
                service_name,
                upstream_base_url,
                "http",
                attempt,
                internal_context.is_some(),
            );
            let result = http::handler(
                req,
                &uri,
                &client,
                &upstream_headers,
                internal_dispatch.as_ref(),
                &runtime.settings,
                &state.execution,
            )
            .instrument(observation.span.clone())
            .await;
            observation.record_result(&runtime.core.balancers, &result);
            result
        }
    }
}

fn prepare_internal_dispatch(
    service: &str,
    settings: Option<&InternalContextNode>,
    state: &RequestState,
    attempt: DispatchAttempt,
) -> Result<Option<InternalDispatch>, ErrorResponse> {
    settings
        .map(|settings| {
            InternalDispatch::new(
                service,
                &settings.audience,
                state.propagation_draft.as_ref(),
                state.internal_context_runtime.as_ref(),
                attempt,
            )
        })
        .transpose()
}

fn execute_direct_response(
    status: u16,
    headers: &[HeaderValueNode],
    body: Option<&ResponseBodyNode>,
) -> Result<Response, ErrorResponse> {
    let started = Instant::now();
    let result = build_direct_response(status, headers, body);
    record_attempt_metrics(
        "direct_response",
        "direct_response",
        "http",
        &result,
        started.elapsed(),
    );
    result
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

#[cfg(test)]
mod disposal_tests;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub(super) enum SelectedService {
    Upstream {
        service_name: String,
        upstream_base_url: String,
        internal_context: Option<InternalContextNode>,
        forward_credentials: bool,
    },
    DirectResponse {
        status: u16,
        headers: Vec<HeaderValueNode>,
        body: Option<ResponseBodyNode>,
    },
}
