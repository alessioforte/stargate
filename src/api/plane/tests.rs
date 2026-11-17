use crate::{
    api::server_router, conf::hosts::IamHosts, gate::test_support, srv::health::Lifecycle,
};
use axum::{Extension, Router, body::Body, response::Response, routing::get};
use http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use std::sync::Arc;
use tokio::{net::TcpListener, task::JoinHandle};
use tower::ServiceExt;

struct Upstream {
    url: String,
    task: JoinHandle<()>,
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Upstream {
    async fn start() -> Self {
        let app = Router::new().route("/payload", get(payload)).fallback(echo);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { url, task }
    }
}

/// Reports the method, the exact path, and the body length the upstream received.
async fn echo(request: axum::extract::Request) -> String {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .unwrap();
    format!("{method} {path} {}", body.len())
}

async fn payload() -> String {
    "stargate".repeat(512)
}

fn config(url: &str) -> gate::cfg::Config {
    serde_json::from_value(serde_json::json!({
        "schema": gate::cfg::SCHEMA,
        "ingress": {"limit": "ingress", "timeout": "250ms"},
        "limits": {
            "default": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "1h"}},
            "ingress": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "1h"}},
        },
        "http": {
            "upstreams": {"backend": {"targets": [{"url": url}]}},
            "services": {"backend": {"kind": "load_balancer", "upstream": "backend"}},
            "routers": {"backend": {"match": {"path": {"prefix": "/"}}, "service": "backend"}},
        },
    }))
    .unwrap()
}

fn app_for(upstream: &Upstream, hosts: IamHosts) -> Router {
    server_router(Lifecycle::default(), hosts).layer(Extension(Arc::new(test_support::gate(
        config(&upstream.url),
    ))))
}

fn request(method: Method, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

fn preflight(path: &str, requested: &str) -> Request<Body> {
    Request::builder()
        .method(Method::OPTIONS)
        .uri(path)
        .header(header::ORIGIN, "https://app.example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, requested)
        .body(Body::empty())
        .unwrap()
}

fn gzip(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header(header::ACCEPT_ENCODING, "gzip")
        .body(Body::empty())
        .unwrap()
}

async fn text(response: Response) -> String {
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn gateway_hands_paths_to_the_upstream_unchanged() {
    let upstream = Upstream::start().await;
    let app = app_for(&upstream, IamHosts::default());
    for path in ["/assets/", "/docs/terms/"] {
        let response = app
            .clone()
            .oneshot(request(Method::GET, path))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(text(response).await, format!("GET {path} 0"), "{path}");
    }
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/upload/")
                .body(Body::from("hello"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(text(response).await, "POST /upload/ 5");
}

#[tokio::test]
async fn hosts_outside_the_iam_planes_reach_the_gateway_with_the_client_path() {
    let upstream = Upstream::start().await;
    let hosts =
        IamHosts::parse(Some("identity.example.com"), "https://identity.example.com").unwrap();
    let app = app_for(&upstream, hosts);
    for path in ["/assets/", "/docs/"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::HOST, "api.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(text(response).await, format!("GET {path} 0"), "{path}");
    }
}

#[tokio::test]
async fn gateway_preflights_reach_the_upstream() {
    let upstream = Upstream::start().await;
    let response = app_for(&upstream, IamHosts::default())
        .oneshot(preflight("/assets/", "POST"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        !response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
    assert_eq!(text(response).await, "OPTIONS /assets/ 0");
}

#[tokio::test]
async fn proxied_bodies_are_not_compressed_but_iam_responses_are() {
    let upstream = Upstream::start().await;
    let app = app_for(&upstream, IamHosts::default());

    let response = app.clone().oneshot(gzip("/payload")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
    assert_eq!(text(response).await.len(), "stargate".len() * 512);

    let response = app.oneshot(gzip("/docs")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_ENCODING], "gzip");
}

#[tokio::test]
async fn iam_routes_keep_normalized_paths_and_preflights() {
    let upstream = Upstream::start().await;
    let app = app_for(&upstream, IamHosts::default());

    let response = app
        .clone()
        .oneshot(request(Method::GET, "/docs/"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let document: serde_json::Value = serde_json::from_str(&text(response).await).unwrap();
    assert!(document["openapi"].is_string());

    let response = app.oneshot(preflight("/docs", "GET")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_METHODS)
    );
}
