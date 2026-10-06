pub(in crate::api::gateway) mod disposal;
pub(in crate::api::gateway) mod mirrors;
pub(in crate::api::gateway) mod plan;
pub(in crate::api::gateway) mod replay;

use self::{
    disposal::discard_response,
    mirrors::{admit_mirrors, record_skipped_mirrors, spawn_mirrors},
    plan::{ExecutionPlan, ServiceSelectionError, selection_error_response},
    replay::{ReplayEligibility, ReplayRequest, buffer_request, content_length_exceeds},
};
use super::{
    request::RequestState,
    upstream::{
        SelectedService, attempt::next_dispatch_attempt, execute_selected_from_replay,
        execute_selected_with_request,
    },
};
use crate::{
    err::{ErrorCode, ErrorResponse},
    etc::{gate::RuntimeSnapshot, http::request::RequestExt, observability::telemetry},
};
use axum::{body::Body, response::Response};
use ctx::DispatchKind;
use futures_util::future::BoxFuture;
use http::{Method, Request};
use std::sync::Arc;

pub(super) async fn execute_plan_with_request(
    runtime: &Arc<RuntimeSnapshot>,
    plan: &ExecutionPlan,
    request: Request<Body>,
    state: &RequestState,
) -> Result<Response, ErrorResponse> {
    let websocket = request.get_protocol() == "ws";
    let mut executor = PlanExecutor {
        runtime,
        state,
        method: request.method().clone(),
        request: Some(request),
        replay: None,
        dispatch_kind: DispatchKind::Primary,
        network_attempt: 0,
        replay_allowed: !websocket,
        mirrors: Vec::new(),
        pending_response: None,
    };
    executor.execute(plan, false).await
}

pub(super) fn execute_plan_from_replay<'a>(
    runtime: &'a Arc<RuntimeSnapshot>,
    plan: &'a ExecutionPlan,
    replay: &'a ReplayRequest,
    state: &'a RequestState,
    dispatch_kind: DispatchKind,
) -> BoxFuture<'a, Result<Response, ErrorResponse>> {
    Box::pin(async move {
        let mut executor = PlanExecutor {
            runtime,
            state,
            method: replay.method.clone(),
            request: None,
            replay: Some(replay.clone()),
            dispatch_kind,
            network_attempt: 0,
            replay_allowed: true,
            mirrors: Vec::new(),
            pending_response: None,
        };
        executor.execute(plan, false).await
    })
}

struct PlanExecutor<'a> {
    runtime: &'a Arc<RuntimeSnapshot>,
    state: &'a RequestState,
    method: Method,
    request: Option<Request<Body>>,
    replay: Option<ReplayRequest>,
    dispatch_kind: DispatchKind,
    network_attempt: u16,
    replay_allowed: bool,
    mirrors: Vec<ExecutionPlan>,
    // Keep the last response if every remaining branch is unavailable. Dispose
    // it only after selecting a leaf in the next branch, before dispatching it.
    pending_response: Option<Response>,
}

impl PlanExecutor<'_> {
    fn can_failover(&self) -> bool {
        self.replay_allowed
            && ReplayEligibility::for_method(&self.method) == ReplayEligibility::Idempotent
    }

    fn execute<'a>(
        &'a mut self,
        plan: &'a ExecutionPlan,
        may_failover: bool,
    ) -> BoxFuture<'a, Result<Response, ErrorResponse>> {
        Box::pin(async move {
            self.state.execution.check()?;
            match plan {
                ExecutionPlan::Failover {
                    services,
                    on_status,
                } => {
                    self.execute_failover(services, on_status, may_failover)
                        .await
                }
                ExecutionPlan::Mirror { service, mirrors } => {
                    let previous = self.mirrors.len();
                    if self.replay_allowed {
                        self.mirrors.extend(mirrors.iter().cloned());
                    }
                    let result = self.execute(service, may_failover).await;
                    // An unavailable main branch must not transfer its mirrors
                    // to a sibling fallback. Entered mirrors are consumed once.
                    self.mirrors.truncate(previous);
                    result
                }
                ExecutionPlan::Upstream {
                    service_name,
                    internal_context,
                } => {
                    let balancer =
                        self.runtime
                            .core
                            .balancers
                            .get(service_name)
                            .ok_or_else(|| {
                                ErrorResponse::internal(format!(
                                    "Load balancer '{service_name}' not found"
                                ))
                            })?;
                    let upstream = balancer
                        .select(&lb::RequestContext {
                            client_ip: &self.state.client_ip,
                            path: &self.state.path,
                            method: self.method.as_str(),
                            key: self.state.load_balancer_key.as_deref(),
                        })
                        .ok_or_else(no_healthy_upstream)?;
                    let selected = SelectedService::Upstream {
                        service_name: service_name.clone(),
                        upstream_base_url: upstream.base_url.clone(),
                        internal_context: internal_context.clone(),
                    };
                    self.execute_leaf(selected, may_failover && self.can_failover())
                        .await
                }
                ExecutionPlan::DirectResponse {
                    status,
                    headers,
                    body,
                } => {
                    self.execute_leaf(
                        SelectedService::DirectResponse {
                            status: *status,
                            headers: headers.clone(),
                            body: body.clone(),
                        },
                        false,
                    )
                    .await
                }
            }
        })
    }

    async fn execute_failover(
        &mut self,
        services: &[ExecutionPlan],
        on_status: &[u16],
        may_failover: bool,
    ) -> Result<Response, ErrorResponse> {
        let mut last_error = None;
        let mut owns_pending_response = false;
        for (index, service) in services.iter().enumerate() {
            let has_fallback = index + 1 < services.len();
            let result = self.execute(service, may_failover || has_fallback).await;
            match result {
                Err(error) if error.code == ErrorCode::GatewayNoHealthyUpstream => continue,
                Err(error)
                    if self.can_failover()
                        && has_fallback
                        && error.code == ErrorCode::UpstreamConnectionFailed =>
                {
                    telemetry::record_gateway_failover("transport", None);
                    tracing::warn!("Failing over after upstream transport error");
                    last_error = Some(error);
                    owns_pending_response = false;
                }
                Ok(response)
                    if self.can_failover()
                        && has_fallback
                        && on_status.contains(&response.status().as_u16()) =>
                {
                    let status = response.status().as_u16();
                    telemetry::record_gateway_failover("status", Some(status));
                    tracing::warn!(status, "Failing over response by status");
                    self.pending_response = Some(response);
                    owns_pending_response = true;
                    last_error = None;
                }
                result => return result,
            }
        }
        if owns_pending_response {
            return self.pending_response.take().ok_or_else(no_healthy_upstream);
        }
        Err(last_error.unwrap_or_else(no_healthy_upstream))
    }

    async fn execute_leaf(
        &mut self,
        selected: SelectedService,
        needs_replay: bool,
    ) -> Result<Response, ErrorResponse> {
        let mirrors = admit_mirrors(self.runtime, &std::mem::take(&mut self.mirrors));
        if self.replay.is_none() && (needs_replay || !mirrors.is_empty()) {
            let request = self
                .request
                .as_ref()
                .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed))?;
            let limit = self.runtime.settings.replay_body_bytes;
            let reservation = if !needs_replay && content_length_exceeds(request.headers(), limit) {
                record_skipped_mirrors(&mirrors, "skipped_payload");
                None
            } else {
                match self.runtime.resources.reserve_replay(limit) {
                    Ok(reservation) => Some(reservation),
                    Err(_) if !needs_replay => {
                        record_skipped_mirrors(&mirrors, "skipped_memory");
                        None
                    }
                    Err(error) => return Err(error),
                }
            };
            if let Some(reservation) = reservation {
                let request = self.request.take().ok_or_else(|| {
                    ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed)
                })?;
                let replay =
                    buffer_request(request, self.runtime, &self.state.execution, reservation)
                        .await?;
                telemetry::record_gateway_replay_bytes(replay.body.len());
                self.replay = Some(replay);
            }
        }
        if let Some(response) = self.pending_response.take() {
            discard_response(
                response,
                "failover",
                &self.runtime.settings,
                &self.state.execution,
            )
            .await;
        }
        if let Some(replay) = &self.replay {
            spawn_mirrors(
                self.runtime.clone(),
                mirrors,
                replay.clone(),
                self.state.clone(),
            );
            let attempt =
                next_dispatch_attempt(&selected, self.dispatch_kind, &mut self.network_attempt)?;
            return execute_selected_from_replay(
                self.runtime,
                &selected,
                replay,
                self.state,
                attempt,
            )
            .await;
        }
        drop(mirrors);
        // A local response leaves the original body available if an enclosing
        // failover later enters an upstream branch.
        let request = if matches!(selected, SelectedService::DirectResponse { .. }) {
            Request::new(Body::empty())
        } else {
            self.request
                .take()
                .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed))?
        };
        execute_selected_with_request(self.runtime, &selected, request, self.state).await
    }
}

fn no_healthy_upstream() -> ErrorResponse {
    selection_error_response(ServiceSelectionError::NoHealthyUpstream)
}
