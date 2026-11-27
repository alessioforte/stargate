//! Gateway request hygiene: hop-by-hop fields, query tokens, and the IAM
//! session cookie.

use super::replay_support::{Outcome, Upstream};
use crate::gate::authentication::TestAuthenticator;
use crate::{
    conf::hosts::IamHost,
    gate::{Gate, test_support},
    iam::auth::{
        identity::VerifiedIdentity,
        subject::{Subject, SubjectType},
    },
    web::request::RequestExt,
};
use axum::{body::Body, response::Response};
use gate::cfg::Config;
use http::{HeaderMap, Request, StatusCode, header::HOST};
use serde_json::json;
use std::sync::Arc;

const HOST_NAME: &str = "app.test";

fn config(upstream: &str, extra: serde_json::Value) -> Config {
    let mut value = json!({
        "schema": gate::cfg::SCHEMA,
        "ingress": {"limit": "ingress", "timeout": "250ms"},
        "limits": {
            "default": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "1h"}},
            "ingress": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "1h"}}
        },
        "runtime": {"request_timeout": "2s"},
        "http": {
            "upstreams": {"origin": {"targets": [{"url": upstream}]}},
            "services": {"origin": {"kind": "load_balancer", "upstream": "origin"}},
            "routers": {"origin": {"match": {"path": {"prefix": "/"}}, "service": "origin"}}
        }
    });
    let target = value["http"].as_object_mut().unwrap();
    for (key, value) in extra.as_object().unwrap() {
        target.insert(key.clone(), value.clone());
    }
    serde_json::from_value(value).unwrap()
}

fn request(gate: &Arc<Gate>, path: &str) -> Request<Body> {
    let mut request = Request::builder()
        .uri(path)
        .header(HOST, HOST_NAME)
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(gate.clone());
    request.extensions_mut().insert(crate::srv::test_connection(
        "203.0.113.10:12345".parse().unwrap(),
    ));
    crate::srv::client_ip::prepare(&mut request);
    request
}

async fn send(request: Request<Body>) -> Response {
    crate::gate::service(request).await.unwrap()
}

fn user() -> VerifiedIdentity {
    VerifiedIdentity::test_jwt(
        Subject::new("01JZ000000000000000000000A".into(), SubjectType::User, None),
        "01JZ000000000000000000000B".into(),
        None,
    )
}

#[tokio::test]
async fn a_client_connection_header_cannot_remove_an_operator_header() {
    let upstream = Upstream::start(Outcome::Status(200)).await;
    let gate = Arc::new(test_support::gate(config(
        &upstream.url,
        json!({
            "middlewares": {
                "tenant": {"kind": "request_headers", "set": [{"name": "x-tenant-id", "value": "operator"}]}
            },
            "routers": {
                "origin": {
                    "match": {"path": {"prefix": "/"}},
                    "service": "origin",
                    "middlewares": ["tenant"]
                }
            }
        }),
    )));

    let mut request = request(&gate, "/orders");
    request
        .headers_mut()
        .insert(http::header::CONNECTION, "x-tenant-id".parse().unwrap());
    request
        .headers_mut()
        .insert("x-tenant-id", "client".parse().unwrap());

    let response = send(request).await;
    assert_eq!(response.status(), StatusCode::OK);

    let records = upstream.records();
    assert_eq!(records.len(), 1);
    let values = records[0]
        .headers
        .get_all("x-tenant-id")
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(values, ["operator"]);
}

#[tokio::test]
async fn query_tokens_are_read_only_where_the_auth_policy_opts_in() {
    let upstream = Upstream::start(Outcome::Status(200)).await;
    let gate = Arc::new(test_support::gate(config(
        &upstream.url,
        json!({
            "policies": {
                "query": {"kind": "auth", "strategies": ["jwt"], "allow_query_token": true},
                "bearer": {"kind": "auth", "strategies": ["jwt"]}
            },
            "routers": {
                "query": {"match": {"path": {"exact": "/query"}}, "service": "origin", "policies": ["query"]},
                "bearer": {"match": {"path": {"exact": "/bearer"}}, "service": "origin", "policies": ["bearer"]}
            }
        }),
    )));
    let authenticator = TestAuthenticator {
        verify: Arc::new(|request, allow_query_token| {
            if request.get_gateway_token(allow_query_token).as_deref() == Some("qtok") {
                user()
            } else {
                VerifiedIdentity::anonymous()
            }
        }),
    };
    let request = |path: &str| {
        let mut request = request(&gate, path);
        request.extensions_mut().insert(authenticator.clone());
        request
    };

    let allowed = send(request("/query?token=qtok")).await;
    assert_eq!(allowed.status(), StatusCode::OK);

    let denied = send(request("/bearer?token=qtok")).await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_iam_host_drops_upstream_session_cookies() {
    let mut headers = HeaderMap::new();
    headers.append(
        http::header::SET_COOKIE,
        "jwt=planted; Path=/; HttpOnly".parse().unwrap(),
    );
    headers.append(
        http::header::SET_COOKIE,
        "app=1; Path=/; SameSite=Lax".parse().unwrap(),
    );
    let upstream = Upstream::start_with(Outcome::Status(204), headers).await;
    let gate = Arc::new(test_support::gate(config(&upstream.url, json!({}))));

    let response = send(request(&gate, "/")).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookies = response
        .headers()
        .get_all(http::header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        cookies,
        [
            "jwt=planted; Path=/; HttpOnly",
            "app=1; Path=/; SameSite=Lax"
        ]
    );

    let mut iam_request = request(&gate, "/");
    iam_request.extensions_mut().insert(IamHost);
    let response = send(iam_request).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookies = response
        .headers()
        .get_all(http::header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(cookies, ["app=1; Path=/; SameSite=Lax"]);
}
