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
};
use crate::{
    err::{ErrorCode, ErrorResponse},
    etc::{
        auth::identity::{self, VerifiedIdentity},
        gate::Gate,
        http::request::RequestExt,
        internal_context::InternalContextRuntime,
        observability::telemetry,
        request_context::{self, PropagationDraft},
    },
};
use ::http::{HeaderMap, HeaderName, HeaderValue, Method, Request};
use axum::{body::Body, response::Response};
use gate::{PolicySnapshot, graph::RouterNode};
use std::{sync::Arc, time::Instant};
use tracing::Instrument;

#[derive(Debug, Clone)]
pub(super) struct RequestState {
    pub(super) execution: crate::api::gateway::lifecycle::Execution,
    pub(super) client_ip: String,
    pub(super) load_balancer_key: Option<String>,
    pub(super) original_path: String,
    pub(super) path: String,
    pub(super) query: String,
    pub(super) preserve_host: bool,
    pub(super) response_headers: ResponseHeaderMutations,
    pub(super) propagation_draft: Option<Arc<PropagationDraft>>,
    pub(super) internal_context_runtime: Option<Arc<InternalContextRuntime>>,
}

#[cfg(all(test, feature = "memory"))]
#[derive(Clone)]
pub(crate) struct IngressPause {
    pub pinned: Arc<tokio::sync::Barrier>,
    pub resume: Arc<tokio::sync::Barrier>,
}

pub(super) async fn handle_hyper(mut req: Request<Body>) -> Result<Response, ErrorResponse> {
    let started = Instant::now();
    let mut router_name = "unmatched".to_string();
    let mut service_name = "unmatched".to_string();

    let result: Result<Response, ErrorResponse> = async {
        let gate = req
            .extensions()
            .get::<Arc<Gate>>()
            .cloned()
            .expect("Gate extension must be configured");

        let runtime = gate.snapshot();
        let policies = gate.policy_snapshot.load_full();
        let admission = runtime.resources.admit_primary()?;
        let execution = lifecycle::Execution::new(
            runtime.settings.request_timeout,
            admission.cancellation.clone(),
        );
        req.extensions_mut().insert(admission.clone());
        execution
            .run(
                "total",
                runtime.settings.request_timeout,
                handle_request(
                    req,
                    runtime.clone(),
                    policies,
                    &execution,
                    admission,
                    &mut router_name,
                    &mut service_name,
                ),
            )
            .await?
    }
    .await;

    let status = match &result {
        Ok(response) => response.status().as_u16(),
        Err(error) => error.status.as_u16(),
    };
    telemetry::record_gateway_request(&router_name, &service_name, status, started.elapsed());
    if status >= 500 {
        tracing::Span::current().record("otel.status_code", "ERROR");
    }

    result
}

async fn handle_request(
    mut req: Request<Body>,
    runtime: Arc<crate::etc::gate::RuntimeSnapshot>,
    policies: Arc<gate::PolicySnapshot>,
    execution: &lifecycle::Execution,
    admission: Arc<crate::etc::gate::resources::Admission>,
    router_name: &mut String,
    service_name: &mut String,
) -> Result<Response, ErrorResponse> {
    #[cfg(all(test, feature = "memory"))]
    if let Some(pause) = req.extensions().get::<IngressPause>().cloned() {
        pause.pinned.wait().await;
        pause.resume.wait().await;
    }
    let graph = &runtime.core.graph;

    let client_ip = req.get_client_ip();
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
    *router_name = router.name.clone();
    *service_name = router.service.clone();
    tracing::Span::current().record("stargate.router", router_name.as_str());
    tracing::Span::current().record("stargate.service", service_name.as_str());
    tracing::Span::current().record("stargate.config_version", runtime.version);

    let method = req.method().clone();
    let mut state = RequestState {
        execution: {
            let mut execution = execution.clone();
            execution.stream = router.response_mode == gate::cfg::ResponseMode::Stream;
            execution
        },
        client_ip: client_ip.clone(),
        load_balancer_key: sub.map(|sub| sub.id.clone()),
        original_path: req.uri().path().to_string(),
        path: req.uri().path().to_string(),
        query: req.uri().query().unwrap_or("").to_string(),
        preserve_host: false,
        response_headers: ResponseHeaderMutations::default(),
        propagation_draft: None,
        internal_context_runtime: crate::etc::internal_context::runtime().cloned(),
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

    let request_id = request_context::request_id_from(req.extensions());

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
            runtime.settings.response_body_idle_timeout,
            "response_body",
            Some(admission),
        );
        crate::etc::gate::retain_runtime(body, runtime.clone())
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
    let Some(request_context) = req.extensions().get::<request_context::RequestContext>() else {
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
