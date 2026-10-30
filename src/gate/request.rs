use super::{
    authentication,
    execution::{
        execute_plan_with_request,
        plan::{build_execution_plan, selection_error_response},
    },
    lifecycle,
    middlewares::apply_middlewares,
    policies::{apply_policies, ingress},
    response::{
        ResponseHeaderMutations, apply_gateway_headers, apply_response_header_mutations,
        strip_internal_context_response,
    },
    routing::router_matches,
    upstream::headers::{UpstreamHeaders, strip_untrusted_forwarded_headers},
};
use crate::{
    ctx::{PropagationDraft, RequestContext, request_id_from, sign::InternalContextRuntime},
    err::{ErrorCode, ErrorResponse},
    gate::{Gate, resources::Admission},
    iam::auth::identity::{self, VerifiedIdentity},
    obs::telemetry,
    web::request::RequestExt,
};
use ::http::{HeaderMap, HeaderName, HeaderValue, Method, Request};
use axum::{body::Body, response::Response};
use gate::{PolicySnapshot, graph::RouterNode};
use std::{sync::Arc, time::Instant};
use tracing::Instrument;

#[derive(Debug, Clone)]
pub(super) struct RequestState {
    pub(super) execution: crate::gate::lifecycle::Execution,
    pub(super) client_ip: String,
    pub(super) load_balancer_key: Option<String>,
    pub(super) original_path: String,
    pub(super) path: String,
    pub(super) query: String,
    pub(super) upstream_headers: UpstreamHeaders,
    pub(super) response_headers: ResponseHeaderMutations,
    pub(super) propagation_draft: Option<Arc<PropagationDraft>>,
    pub(super) internal_context_runtime: Option<Arc<InternalContextRuntime>>,
}

struct RequestDeadlines {
    started: tokio::time::Instant,
    grpc_deadline: Option<tokio::time::Instant>,
    updates: tokio::sync::watch::Sender<Option<tokio::time::Instant>>,
}

struct RouteLabels {
    router: String,
    service: String,
}

#[cfg(all(test, feature = "memory"))]
#[derive(Clone)]
pub(crate) struct IngressPause {
    pub pinned: Arc<tokio::sync::Barrier>,
    pub resume: Arc<tokio::sync::Barrier>,
}

pub(super) async fn handle_hyper(mut req: Request<Body>) -> Result<Response, ErrorResponse> {
    let started = Instant::now();
    let grpc_request = super::grpc::is_grpc_request(req.headers());
    let mut labels = RouteLabels {
        router: "unmatched".to_string(),
        service: "unmatched".to_string(),
    };

    let result: Result<Response, ErrorResponse> = async {
        let grpc_timeout = if grpc_request {
            super::grpc::timeout(req.headers())?
        } else {
            None
        };
        let gate = req
            .extensions()
            .get::<Arc<Gate>>()
            .cloned()
            .expect("Gate extension must be configured");

        let runtime = gate.snapshot();
        let policies = gate.policy_snapshot.load_full();
        let admission = runtime.resources.admit_primary()?;
        let request_started = tokio::time::Instant::now();
        let mut execution = lifecycle::Execution::new(
            runtime.settings.request_timeout,
            admission.cancellation.clone(),
        );
        execution.grpc = grpc_request;
        let grpc_deadline = grpc_timeout.and_then(|timeout| request_started.checked_add(timeout));
        if let Some(deadline) = grpc_deadline {
            execution.deadline = Some(
                execution
                    .deadline
                    .map_or(deadline, |current| current.min(deadline)),
            );
        }
        let (deadlines, updates) = tokio::sync::watch::channel(execution.deadline);
        req.extensions_mut().insert(admission.clone());
        execution
            .run_with_deadline_updates(
                updates,
                handle_request(
                    req,
                    runtime.clone(),
                    policies,
                    &execution,
                    RequestDeadlines {
                        started: request_started,
                        grpc_deadline,
                        updates: deadlines,
                    },
                    admission,
                    &mut labels,
                ),
            )
            .await?
    }
    .await;

    let status = match &result {
        Ok(response) => response.status().as_u16(),
        Err(error) => error.status.as_u16(),
    };
    telemetry::record_gateway_request(&labels.router, &labels.service, status, started.elapsed());
    if status >= 500 {
        tracing::Span::current().record("otel.status_code", "ERROR");
    }

    if grpc_request {
        match result {
            Ok(response) => Ok(super::grpc::observe_response(
                response,
                labels.router,
                labels.service,
            )),
            Err(error) => {
                let status = super::grpc::status_for_http(error.status);
                telemetry::record_gateway_grpc_response(&labels.router, &labels.service, status);
                Ok(super::grpc::error_response(error))
            }
        }
    } else {
        result
    }
}

async fn handle_request(
    mut req: Request<Body>,
    runtime: Arc<crate::gate::RuntimeSnapshot>,
    policies: Arc<gate::PolicySnapshot>,
    execution: &lifecycle::Execution,
    deadlines: RequestDeadlines,
    admission: Arc<Admission>,
    labels: &mut RouteLabels,
) -> Result<Response, ErrorResponse> {
    #[cfg(all(test, feature = "memory"))]
    if let Some(pause) = req.extensions().get::<IngressPause>().cloned() {
        pause.pinned.wait().await;
        pause.resume.wait().await;
    }
    let graph = &runtime.core.graph;

    let client_ip = req.get_client_ip();
    let upstream_headers =
        UpstreamHeaders::from_request(&req, crate::srv::tls::enabled().unwrap_or(false));
    strip_untrusted_forwarded_headers(req.headers_mut(), upstream_headers.peer);
    ingress::apply_ingress_limit(&runtime, &client_ip, execution)
        .instrument(tracing::debug_span!("gateway.ingress"))
        .await?;
    let identity = authentication::authenticate(authentication::auth_request(&req))
        .instrument(tracing::debug_span!("gateway.authenticate"))
        .await;
    let auth_kind = identity.auth_kind();
    let sub = identity.subject();
    tracing::Span::current().record(
        "stargate.auth_kind",
        match auth_kind {
            identity::AuthKind::ApiKey => "api_key",
            identity::AuthKind::Jwt => "jwt",
            identity::AuthKind::OAuth => "oauth",
            identity::AuthKind::Anonymous => "anonymous",
        },
    );

    let router = {
        let _span = tracing::debug_span!("gateway.route_match").entered();
        graph
            .routers
            .iter()
            .find(|router| router_matches(router, &req))
            .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayRouteNotFound))?
    };
    labels.router = router.name.clone();
    labels.service = router.service.clone();
    tracing::Span::current().record("stargate.router", labels.router.as_str());
    tracing::Span::current().record("stargate.service", labels.service.as_str());
    tracing::Span::current().record("stargate.config_version", runtime.version);

    let router_deadline = match router.request_timeout {
        gate::graph::TimeoutOverride::Inherit => deadlines
            .started
            .checked_add(runtime.settings.request_timeout),
        gate::graph::TimeoutOverride::Disabled => None,
        gate::graph::TimeoutOverride::Duration(timeout) => deadlines.started.checked_add(timeout),
    };
    let deadline = match (router_deadline, deadlines.grpc_deadline) {
        (Some(router), Some(client)) => Some(router.min(client)),
        (Some(deadline), None) | (None, Some(deadline)) => Some(deadline),
        (None, None) => None,
    };
    deadlines.updates.send_replace(deadline);

    let method = req.method().clone();
    let mut state = RequestState {
        execution: {
            let mut execution = execution.clone();
            execution.stream = router.response_mode == gate::cfg::ResponseMode::Stream;
            execution.deadline = deadline;
            execution.preserve_stream_deadline = execution.grpc
                || !matches!(
                    router.request_timeout,
                    gate::graph::TimeoutOverride::Inherit
                );
            execution.upload_idle_override = router.upload_idle_timeout;
            execution
        },
        client_ip: client_ip.clone(),
        load_balancer_key: sub.map(|sub| sub.id.clone()),
        original_path: req.uri().path().to_string(),
        path: req.uri().path().to_string(),
        query: req.uri().query().unwrap_or("").to_string(),
        upstream_headers,
        response_headers: ResponseHeaderMutations::default(),
        propagation_draft: None,
        internal_context_runtime: crate::ctx::sign::runtime().cloned(),
    };

    {
        let _span = tracing::debug_span!(
            "gateway.apply_middlewares",
            stargate.router = %router.name,
            stargate.service = %router.service,
        )
        .entered();
        apply_middlewares(graph, router, &mut req, &mut state)?;
    }

    let request_id = request_id_from(req.extensions());

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(&request_id).unwrap(),
    );

    apply_policies(
        router,
        &runtime,
        &policies,
        &mut req,
        &identity,
        &client_ip,
        &mut response_headers,
    )
    .instrument(tracing::debug_span!(
        "gateway.apply_policies",
        stargate.router = %router.name,
        stargate.service = %router.service,
    ))
    .await?;

    state.propagation_draft =
        capture_propagation_draft(&req, &identity, &method, &state, router, &policies);

    let plan = {
        let _span = tracing::debug_span!(
            "gateway.build_execution_plan",
            stargate.router = %router.name,
            stargate.service = %router.service,
        )
        .entered();
        build_execution_plan(graph, &router.service, &request_id)
            .map_err(selection_error_response)?
    };

    let mut response = execute_plan_with_request(&runtime, &plan, req, &state)
        .instrument(tracing::info_span!(
            "gateway.execute",
            stargate.router = %router.name,
            stargate.service = %router.service,
        ))
        .await?;

    // This is global gateway-owned response hygiene, including upstreams
    // without internal context and direct responses.
    strip_internal_context_response(response.headers_mut());
    apply_response_header_mutations(response.headers_mut(), &state.response_headers);
    apply_gateway_headers(response.headers_mut(), &response_headers);
    Ok(response.map(|body| {
        let body = lifecycle::body::guarded_body(
            body,
            state.execution.response(),
            Some(runtime.settings.response_body_idle_timeout),
            "response_body",
            Some(admission),
            if state.execution.grpc {
                lifecycle::body::Termination::GrpcTrailers
            } else {
                lifecycle::body::Termination::Error
            },
        );
        crate::gate::retain_runtime(body, runtime.clone())
    }))
}

/// Capture trusted facts after middleware and policy success. Rejection remains
/// optional here; a signed upstream fails during its own dispatch preparation.
fn capture_propagation_draft(
    req: &Request<Body>,
    identity: &VerifiedIdentity,
    method: &Method,
    state: &RequestState,
    router: &RouterNode,
    policies: &PolicySnapshot,
) -> Option<Arc<PropagationDraft>> {
    let Some(request_context) = req.extensions().get::<RequestContext>() else {
        telemetry::record_propagation_draft("rejected", "request_context");
        tracing::warn!(reason = "request_context", "Propagation draft rejected");
        return None;
    };
    match PropagationDraft::build(
        identity,
        request_context,
        method.as_str(),
        &state.path,
        &state.original_path,
        ctx::RouteContext {
            router: router.name.clone(),
            service: router.service.clone(),
            policy_revision: Some(policies.revision.clone()),
        },
    ) {
        Ok(draft) => {
            telemetry::record_propagation_draft("created", draft.actor_label());
            Some(Arc::new(draft))
        }
        Err(error) => {
            telemetry::record_propagation_draft("rejected", error.category());
            tracing::warn!(reason = error.category(), "Propagation draft rejected");
            None
        }
    }
}
