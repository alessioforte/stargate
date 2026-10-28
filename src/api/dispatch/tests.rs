use crate::{
    api::server_router, conf::hosts::IamHosts, gate::test_support, srv::health::Lifecycle,
};
use axum::{Extension, Router, body::Body, response::Response};
use http::{Request, StatusCode, Version};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

fn app(hosts: IamHosts, ingress_burst: u64, lifecycle: Lifecycle) -> Router {
    let config = serde_json::from_value(serde_json::json!({
        "schema": gate::cfg::SCHEMA,
        "ingress": {"limit": "ingress", "timeout": "250ms"},
        "limits": {
            "default": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "1h"}},
            "ingress": {"strategy": "gcra", "params": {"max_burst": ingress_burst, "replenish_1_per": "1h"}},
        },
        "http": {
            "services": {"backend": {"kind": "direct_response", "status": 200, "body": {"text": "gateway backend"}}},
            "routers": {"backend": {"match": {"path": {"prefix": "/"}}, "service": "backend"}},
        },
    }))
    .unwrap();
    server_router(lifecycle, hosts).layer(Extension(Arc::new(test_support::gate(config))))
}

fn iam_hosts() -> IamHosts {
    IamHosts::parse(Some("identity.example.com"), "https://identity.example.com").unwrap()
}

fn request(path: &str, host: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().uri(path);
    if let Some(host) = host {
        builder = builder.header("host", host);
    }
    builder.body(Body::empty()).unwrap()
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

async fn assert_openapi(response: Response) {
    assert_eq!(response.status(), StatusCode::OK);
    let document: serde_json::Value = serde_json::from_str(&text(response).await).unwrap();
    assert!(document["openapi"].is_string());
}

#[tokio::test]
async fn unrestricted_hosts_preserve_iam_routes() {
    let app = app(IamHosts::default(), 100, Lifecycle::default());
    for host in [Some("identity.example.com"), Some("api.example.com"), None] {
        assert_openapi(app.clone().oneshot(request("/docs", host)).await.unwrap()).await;
    }
}

#[tokio::test]
async fn configured_hosts_match_case_insensitively_without_the_port() {
    let app = app(iam_hosts(), 100, Lifecycle::default());
    for host in ["identity.example.com", "Identity.Example.com:443"] {
        assert_openapi(
            app.clone()
                .oneshot(request("/docs", Some(host)))
                .await
                .unwrap(),
        )
        .await;
    }
}

#[tokio::test]
async fn http2_authority_selects_the_iam_host() {
    let app = app(iam_hosts(), 100, Lifecycle::default());
    for (host, iam) in [
        ("Identity.Example.com:443", true),
        ("api.example.com", false),
    ] {
        let request = Request::builder()
            .version(Version::HTTP_2)
            .uri(format!("https://{host}/docs"))
            .body(Body::empty())
            .unwrap();
        assert!(!request.headers().contains_key("host"));
        let response = app.clone().oneshot(request).await.unwrap();
        if iam {
            assert_openapi(response).await;
        } else {
            assert_eq!(text(response).await, "gateway backend");
        }
    }
}

#[tokio::test]
async fn other_hosts_bypass_all_iam_paths() {
    let app = app(iam_hosts(), 100, Lifecycle::default());
    for path in [
        "/docs",
        "/docs/errors",
        "/i18n/it",
        "/.well-known/openid-configuration",
        "/account/profile",
        "/oauth/userinfo",
        "/signup",
        "/admin/me",
        "/auth/login",
        "/stargate",
    ] {
        let response = app
            .clone()
            .oneshot(request(path, Some("api.example.com")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(text(response).await, "gateway backend", "{path}");
    }
}

#[tokio::test]
async fn hostless_requests_and_unmatched_iam_paths_reach_the_gateway() {
    let app = app(iam_hosts(), 100, Lifecycle::default());
    for (path, host) in [
        ("/docs", None),
        ("/admin/me", None),
        ("/backend", Some("identity.example.com")),
    ] {
        let response = app.clone().oneshot(request(path, host)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(text(response).await, "gateway backend");
    }
}

#[tokio::test]
async fn each_host_uses_its_own_ingress_path_and_probes_bypass_both() {
    for (host, code) in [
        ("identity.example.com", "iam.ingress_rate_limit_exceeded"),
        ("api.example.com", "gateway.ingress_rate_limit_exceeded"),
    ] {
        let app = app(iam_hosts(), 1, Lifecycle::default());
        let response = app
            .clone()
            .oneshot(request("/docs", Some(host)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = app
            .clone()
            .oneshot(request("/docs", Some(host)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let error: serde_json::Value = serde_json::from_str(&text(response).await).unwrap();
        assert_eq!(error["code"], code);
        for (path, status) in [
            ("/livez", StatusCode::OK),
            ("/readyz", StatusCode::SERVICE_UNAVAILABLE),
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(request(path, Some(host)))
                    .await
                    .unwrap()
                    .status(),
                status
            );
        }
    }
}

#[tokio::test]
async fn probes_respond_on_every_host_when_dependencies_are_unavailable() {
    let lifecycle = Lifecycle::default();
    // No Gate extension: dispatching a probe to either ingress path would panic.
    let app = server_router(lifecycle.clone(), iam_hosts());
    for initialized in [false, true, false] {
        if initialized {
            lifecycle.mark_ready();
        } else {
            lifecycle.begin_shutdown();
        }
        for host in [
            Some("identity.example.com"),
            Some("api.example.com"),
            Some("10.0.0.12:5050"),
            None,
        ] {
            for (path, status, expected) in [
                ("/livez", StatusCode::OK, "alive"),
                ("/readyz", StatusCode::SERVICE_UNAVAILABLE, "not_ready"),
            ] {
                let response = app.clone().oneshot(request(path, host)).await.unwrap();
                assert_eq!(response.status(), status);
                let body: serde_json::Value = serde_json::from_str(&text(response).await).unwrap();
                assert_eq!(body["status"], expected);
            }
        }
    }
}
