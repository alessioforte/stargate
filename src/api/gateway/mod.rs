mod authentication;
mod dispatch;
mod disposal;
mod executor;
mod headers;
mod http;
#[cfg(all(test, feature = "memory"))]
mod ingress_tests;
mod lifecycle;
#[cfg(test)]
mod lifecycle_tests;
mod limits;
mod middlewares;
mod path;
mod planner;
mod policies;
mod replay;
#[cfg(test)]
mod replay_tests;
mod responses;
mod routing;
mod types;
mod ws;

use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::gate::Gate;
use crate::etc::{ext::RequestExt, guard, reqctx, telemetry};
use ::http::{HeaderMap, HeaderName, HeaderValue, Request};
use axum::body::Body;
use axum::response::{IntoResponse, Response};
use executor::{
    admit_mirrors, execute_plan_from_replay, execute_selected_with_request, spawn_mirrors,
};
use headers::{
    apply_gateway_headers, apply_response_header_mutations, strip_internal_context_response,
};
use middlewares::apply_middlewares;
use planner::{build_execution_plan, selection_error_response};
use policies::apply_policies;
use replay::{buffer_request, content_length_exceeds};
use routing::router_matches;
use std::{convert::Infallible, sync::Arc, time::Instant};
use tracing::Instrument;
use types::{ReplayEligibility, RequestState, ResponseHeaderMutations};

#[cfg(all(test, feature = "memory"))]
#[derive(Clone)]
pub(crate) struct IngressPause {
    pub pinned: Arc<tokio::sync::Barrier>,
    pub resume: Arc<tokio::sync::Barrier>,
}

async fn handle_hyper(mut req: Request<Body>) -> Result<Response, ErrorResponse> {
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
    limits::apply_ingress_limit(&runtime, &client_ip, execution)
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
            guard::AuthKind::ApiKey => "api_key",
            guard::AuthKind::Jwt => "jwt",
            guard::AuthKind::OAuth => "oauth",
            guard::AuthKind::Anonymous => "anonymous",
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
        apply_middlewares(&graph, router, &mut req, &mut state)?;
    }

    let request_id = reqctx::request_id_from(req.extensions());

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(&request_id).unwrap(),
    );

    apply_policies(
        &graph,
        router,
        &runtime,
        &policies.engine,
        &policies.revision,
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

    if let Some(request_context) = req.extensions().get::<reqctx::RequestContext>() {
        match reqctx::PropagationDraft::build(
            &identity,
            request_context,
            method.as_str(),
            &state.path,
            &state.original_path,
            &router.name,
            &router.service,
            Some(policies.revision.clone()),
        ) {
            Ok(draft) => {
                telemetry::record_propagation_draft("created", draft.actor_label());
                state.propagation_draft = Some(Arc::new(draft));
            }
            Err(error) => {
                telemetry::record_propagation_draft("rejected", error.category());
                tracing::warn!(reason = error.category(), "Propagation draft rejected");
            }
        }
    } else {
        telemetry::record_propagation_draft("rejected", "request_context");
        tracing::warn!(reason = "request_context", "Propagation draft rejected");
    }

    let ctx = lb::RequestContext {
        client_ip: &client_ip,
        path: &state.path,
        method: method.as_str(),
        key: sub.map(|sub| sub.id.as_str()),
    };

    let balancers = &runtime.core.balancers;
    let plan = {
        let _span = tracing::debug_span!(
            "gateway.build_execution_plan",
            stargate.router = %router.name,
            stargate.service = %router.service,
        )
        .entered();
        build_execution_plan(&graph, balancers, &router.service, &ctx, &request_id)
            .map_err(selection_error_response)?
    };

    let is_websocket = req.get_protocol() == "ws";
    let eligibility = ReplayEligibility::for_method(req.method());
    let needs_failover_replay = plan.needs_failover_replay(eligibility);
    let mirrors = if is_websocket {
        Vec::new()
    } else {
        admit_mirrors(&runtime, &plan.mirrors)
    };
    let mut response = if is_websocket {
        let selected = plan
            .attempts
            .first()
            .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayNoHealthyUpstream))?;
        execute_selected_with_request(&runtime, selected, req, &state)
            .instrument(tracing::info_span!(
                "gateway.execute",
                stargate.router = %router.name,
                stargate.service = %router.service,
            ))
            .await?
    } else if needs_failover_replay || !mirrors.is_empty() {
        let limit = runtime.settings.replay_body_bytes;
        let reservation = if !needs_failover_replay && content_length_exceeds(req.headers(), limit)
        {
            for _ in &mirrors {
                telemetry::record_gateway_mirror("skipped_payload");
            }
            None
        } else {
            match runtime.resources.reserve_replay(limit) {
                Ok(reservation) => Some(reservation),
                Err(_) if !needs_failover_replay => {
                    for _ in &mirrors {
                        telemetry::record_gateway_mirror("skipped_memory");
                    }
                    None
                }
                Err(error) => return Err(error),
            }
        };
        if let Some(reservation) = reservation {
            let replay = buffer_request(req, &runtime, &state.execution, reservation).await?;
            telemetry::record_gateway_replay_bytes(replay.body.len());
            spawn_mirrors(runtime.clone(), mirrors, replay.clone(), state.clone());
            execute_plan_from_replay(&runtime, &plan, &replay, &state, ctx::DispatchKind::Primary)
                .instrument(tracing::info_span!("gateway.execute", stargate.router = %router.name, stargate.service = %router.service))
                .await?
        } else {
            let selected = plan
                .attempts
                .first()
                .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayNoHealthyUpstream))?;
            execute_selected_with_request(&runtime, selected, req, &state)
                .instrument(tracing::info_span!(
                    "gateway.execute",
                    stargate.router = %router.name,
                    stargate.service = %router.service,
                ))
                .await?
        }
    } else {
        let selected = plan
            .attempts
            .first()
            .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayNoHealthyUpstream))?;
        execute_selected_with_request(&runtime, selected, req, &state)
            .instrument(tracing::info_span!(
                "gateway.execute",
                stargate.router = %router.name,
                stargate.service = %router.service,
            ))
            .await?
    };

    // This is global gateway-owned response hygiene, including upstreams
    // without internal context and direct responses.
    strip_internal_context_response(response.headers_mut());
    apply_response_header_mutations(response.headers_mut(), &state.response_headers);
    apply_gateway_headers(response.headers_mut(), &response_headers);
    Ok(response.map(|body| {
        let body = lifecycle::guarded_body(
            body,
            state.execution.response(),
            runtime.settings.response_body_idle_timeout,
            "response_body",
            Some(admission),
        );
        crate::etc::gate::retain_runtime(body, runtime.clone())
    }))
}

pub async fn service(req: Request<Body>) -> Result<Response, Infallible> {
    match handle_hyper(req).await {
        Ok(response) => Ok(response),
        Err(error) => Ok(error.into_response()),
    }
}

#[cfg(test)]
mod tests {
    use super::authentication::auth_request;
    use axum::body::Body;
    use http::Request;

    #[cfg(feature = "memory")]
    use super::service;
    #[cfg(feature = "memory")]
    use http::{StatusCode, header::CONTENT_TYPE};
    #[cfg(feature = "memory")]
    use http_body_util::BodyExt;
    #[cfg(feature = "memory")]
    use std::sync::Arc;

    #[test]
    fn auth_request_preserves_uri_and_headers_without_body() {
        let req = Request::builder()
            .uri("http://api.example.com/reports?preview=true")
            .header("authorization", "Bearer token")
            .header("x-api-key", "api-key")
            .body(Body::from("body must not be copied"))
            .unwrap();

        let auth_req = auth_request(&req);

        assert_eq!(auth_req.uri(), req.uri());
        assert_eq!(auth_req.headers(), req.headers());
        assert_eq!(*auth_req.body(), ());
    }

    #[cfg(feature = "memory")]
    #[tokio::test]
    async fn service_wraps_missing_route_error_as_json_response() {
        let gate = Arc::new(crate::etc::gate::test_support::gate(
            gate::cfg::Config::default(),
        ));
        let mut req = Request::builder()
            .uri("/missing")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(gate);

        let response = service(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "gateway.route_not_found");
        assert_eq!(json["message"], "Gateway route not found");
    }
}
