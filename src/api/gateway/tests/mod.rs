#[cfg(feature = "redis")]
mod cluster;
#[cfg(feature = "memory")]
mod ingress;
mod lifecycle;
mod nested_execution;
mod replay;
mod replay_support;
#[cfg(feature = "memory")]
mod retry_after;

use crate::api::gateway::authentication::auth_request;
use axum::body::Body;
use http::Request;

#[cfg(feature = "memory")]
use crate::api::gateway::service;
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
