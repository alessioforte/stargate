mod executor;
mod headers;
mod http;
mod limits;
mod middlewares;
mod path;
mod planner;
mod policies;
mod replay;
mod responses;
mod routing;
mod types;
mod ws;

use crate::err::{ErrorResponse, HttpError};
use crate::etc::{ext::RequestExt, guard, reqctx};
use ::http::{HeaderMap, HeaderName, HeaderValue, Request};
use axum::body::Body;
use axum::response::{IntoResponse, Response};
use executor::{execute_plan_from_replay, execute_selected_with_request, spawn_mirrors};
use gate::Gate;
use headers::{apply_gateway_headers, apply_response_header_mutations};
use middlewares::apply_middlewares;
use planner::{build_execution_plan, selection_error_response};
use policies::apply_policies;
use replay::{buffer_request, content_length_exceeds, replay_body_limit};
use routing::router_matches;
use std::{convert::Infallible, sync::Arc};
use types::{AuthKind, RequestState, ResponseHeaderMutations};

async fn handle_hyper(mut req: Request<Body>) -> Result<Response, ErrorResponse> {
    let gate = req
        .extensions()
        .get::<Arc<Gate>>()
        .cloned()
        .expect("Gate extension must be configured");

    let auth_req = auth_request(&req);
    let (sub, auth_kind) = match guard::verify_api_key(&auth_req).await {
        Some(subject) => (Some(subject), Some(AuthKind::ApiKey)),
        None => match guard::verify_jwt(&auth_req).await {
            Some(subject) => (Some(subject), Some(AuthKind::Jwt)),
            None => (None, None),
        },
    };

    let graph = gate.http_graph.load_full();

    let router = graph
        .routers
        .iter()
        .find(|router| router_matches(router, &req))
        .ok_or_else(|| ErrorResponse::from(HttpError::NotFound("Route not found".to_string())))?;

    let mut state = RequestState {
        path: req.uri().path().to_string(),
        query: req.uri().query().unwrap_or("").to_string(),
        preserve_host: false,
        response_headers: ResponseHeaderMutations::default(),
    };

    apply_middlewares(&graph, router, &mut req, &mut state)?;

    let request_id = reqctx::request_id_from(req.extensions());
    let client_ip = req.get_client_ip();
    let method = req.method().clone();

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(&request_id).unwrap(),
    );

    apply_policies(
        &graph,
        router,
        &gate,
        &mut req,
        auth_kind,
        sub.as_ref(),
        &client_ip,
        &mut response_headers,
    )
    .await?;

    let ctx = lb::RequestContext {
        client_ip: &client_ip,
        path: &state.path,
        method: method.as_str(),
        key: sub.as_ref().map(|sub| sub.id.as_str()),
    };

    let balancers = gate.http_balancers.load_full();
    let plan = build_execution_plan(
        &graph,
        balancers.as_ref(),
        &router.service,
        &ctx,
        &request_id,
    )
    .map_err(selection_error_response)?;

    let mut response = if req.get_protocol() == "ws" {
        let selected = plan.attempts.first().ok_or_else(|| {
            ErrorResponse::from(HttpError::ServiceUnavailable(
                "No healthy upstream available".to_string(),
            ))
        })?;
        execute_selected_with_request(selected, req, &state).await?
    } else if plan.requires_replay() {
        let limit = replay_body_limit();
        if !plan.needs_status_failover_replay() && content_length_exceeds(req.headers(), limit) {
            tracing::warn!(
                limit,
                "Skipping mirror traffic because request body exceeds replay limit"
            );
            let selected = plan.attempts.first().ok_or_else(|| {
                ErrorResponse::from(HttpError::ServiceUnavailable(
                    "No healthy upstream available".to_string(),
                ))
            })?;
            execute_selected_with_request(selected, req, &state).await?
        } else {
            let replay = buffer_request(req, limit).await?;
            spawn_mirrors(plan.mirrors.clone(), replay.clone(), state.clone());
            execute_plan_from_replay(&plan, &replay, &state).await?
        }
    } else {
        let selected = plan.attempts.first().ok_or_else(|| {
            ErrorResponse::from(HttpError::ServiceUnavailable(
                "No healthy upstream available".to_string(),
            ))
        })?;
        execute_selected_with_request(selected, req, &state).await?
    };

    apply_response_header_mutations(response.headers_mut(), &state.response_headers);
    apply_gateway_headers(response.headers_mut(), &response_headers);
    Ok(response)
}

pub async fn service(req: Request<Body>) -> Result<Response, Infallible> {
    match handle_hyper(req).await {
        Ok(response) => Ok(response),
        Err(error) => Ok(error.into_response()),
    }
}

fn auth_request(req: &Request<Body>) -> Request<()> {
    let mut auth_req = Request::builder().uri(req.uri().clone()).body(()).unwrap();
    *auth_req.headers_mut() = req.headers().clone();
    auth_req
}

#[cfg(test)]
mod tests {
    use super::auth_request;
    use axum::body::Body;
    use http::Request;

    #[cfg(feature = "memory")]
    use super::service;
    #[cfg(feature = "memory")]
    use gate::Gate;
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
        let gate = Arc::new(Gate::new(Arc::new(lim::State::new())));
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
        assert_eq!(json["code"], "not_found");
        assert_eq!(json["message"], "Route not found");
    }
}
