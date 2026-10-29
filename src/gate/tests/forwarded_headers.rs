use super::replay_support::{Outcome, Upstream, state};
use crate::{
    ctx::{PropagationDraft, RequestContext, sign::InternalContextRuntime},
    gate::{
        RuntimeSnapshot,
        execution::{execute_plan_with_request, plan::build_execution_plan},
        request::RequestState,
        upstream::headers::{UpstreamHeaders, strip_untrusted_forwarded_headers},
    },
    iam::auth::identity::VerifiedIdentity,
};
use axum::{Router, body::Body, extract::ConnectInfo, routing::any};
use ctx::{ContextSigner, SignerConfig};
use futures_util::{SinkExt, StreamExt};
use http::{HeaderMap, Request, header::HOST};
use serde_json::json;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::oneshot;
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        handshake::server::{Request as WsRequest, Response as WsResponse},
    },
};

const ORIGINAL_HOST: &str = "frontend.test:8443";
const AUDIENCE: &str = "urn:stargate:service:test";
const HTTP_QUERY: &str = "access_token=a&page=2&jwt=b&access%5Ftoken=c&%74oken=d&keep=%2F";
const WS_QUERY: &str = "token=a&page=2&access%5Ftoken=b&jwt=c&keep=%2F";
const EXTERNAL_COOKIES: &str = "consent=a,b; prefs={\"x\":1}; jwt=session-token; sid=1; flag";

fn runtime(config: serde_json::Value, internal: bool) -> Arc<RuntimeSnapshot> {
    let mut runtime = crate::gate::test_support::runtime(
        gate::cfg::RuntimeConfig::from_raw(serde_json::from_value(config).unwrap()).unwrap(),
    );
    if internal {
        // This fixture supplies its signer directly in RequestState, without
        // configuring the process-wide signer or its file-based preflight.
        for upstream in Arc::get_mut(&mut runtime)
            .unwrap()
            .core
            .graph
            .upstreams
            .values_mut()
        {
            upstream.internal_context = Some(gate::graph::InternalContextNode {
                audience: AUDIENCE.into(),
            });
        }
    }
    runtime
}

fn incoming(uri: &str, internal: bool) -> Request<Body> {
    let mut req = Request::builder().uri(uri).body(Body::empty()).unwrap();
    req.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
    ));
    add_proxy_headers(req.headers_mut(), internal);
    req
}

fn add_proxy_headers(headers: &mut HeaderMap, internal: bool) {
    for (name, value) in [
        ("x-forwarded-for", "1.2.3.4"),
        ("x-forwarded-for", "5.6.7.8"),
        ("x-forwarded-host", "proxy.test:443"),
        ("x-forwarded-proto", "https"),
        ("x-forwarded-port", "443"),
        ("x-forwarded-prefix", "/external"),
        ("x-forwarded-client-cert", "Subject=CN=client"),
        ("x-forwarded-ssl", "on"),
        ("forwarded", "for=1.2.3.4;proto=https"),
        ("x-real-ip", "1.2.3.4"),
        ("authorization", "Bearer client-token"),
        ("x-api-key", "client-key"),
        ("proxy-authorization", "Basic proxy-token"),
        ("cookie", "theme=dark; jwt=session-token; cart=123"),
        ("connection", "x-connection-secret"),
        ("x-connection-secret", "must-not-forward"),
    ] {
        headers.append(name, value.parse().unwrap());
    }
    if !internal {
        headers.insert("cookie", EXTERNAL_COOKIES.parse().unwrap());
    }
}

fn request_state(
    runtime: &RuntimeSnapshot,
    req: &mut Request<Body>,
    internal: bool,
) -> RequestState {
    let mut state = state(runtime);
    state.path = req.uri().path().into();
    state.query = req.uri().query().unwrap_or("").into();
    state.upstream_headers = UpstreamHeaders::from_request(req, false);
    strip_untrusted_forwarded_headers(req.headers_mut(), state.upstream_headers.peer);
    if internal {
        let signer = ContextSigner::from_rsa_pem(
            SignerConfig::new("https://gateway.test/internal", "test", 30).unwrap(),
            crate::ctx::sign::test_support::PRIVATE_KEY,
        )
        .unwrap();
        state.internal_context_runtime = Some(InternalContextRuntime::from_signer_for_test(signer));
        state.propagation_draft = Some(Arc::new(
            PropagationDraft::build(
                &VerifiedIdentity::anonymous(),
                &RequestContext::new(
                    "01JZ000000000000000000000R".into(),
                    chrono::Utc::now(),
                    None,
                    None,
                    None,
                ),
                "GET",
                req.uri().path(),
                req.uri().path(),
                ctx::RouteContext {
                    router: "test".into(),
                    service: "test".into(),
                    policy_revision: None,
                },
            )
            .unwrap(),
        ));
    }
    state
}

fn assert_headers(headers: &HeaderMap, trusted: bool, internal: bool, forward: bool, phase: &str) {
    assert_eq!(headers.get_all("x-forwarded-for").iter().count(), 1);
    assert_eq!(
        headers["x-forwarded-for"],
        if trusted {
            "1.2.3.4, 5.6.7.8, 127.0.0.1"
        } else {
            "127.0.0.1"
        }
    );
    assert_eq!(
        headers["x-forwarded-proto"],
        if trusted { "https" } else { "http" }
    );
    assert_eq!(
        headers["x-forwarded-host"],
        if trusted {
            "proxy.test:443"
        } else {
            ORIGINAL_HOST
        }
    );
    for name in [
        "x-forwarded-port",
        "x-forwarded-prefix",
        "x-forwarded-client-cert",
        "x-forwarded-ssl",
        "forwarded",
        "x-real-ip",
    ] {
        assert_eq!(headers.contains_key(name), trusted, "{name}");
    }
    if trusted {
        assert_eq!(headers["x-forwarded-client-cert"], "Subject=CN=client");
        assert_eq!(headers["x-forwarded-ssl"], "on");
    }
    assert_eq!(headers.contains_key("stargate-context"), internal);
    assert_ne!(headers[HOST], ORIGINAL_HOST);
    assert!(!headers.contains_key("proxy-authorization"));
    assert!(
        !headers.contains_key("x-connection-secret"),
        "connection-nominated field leaked during {phase}"
    );
    if forward {
        assert_eq!(headers["authorization"], "Bearer client-token");
        assert_eq!(headers["x-api-key"], "client-key");
        assert_eq!(headers["cookie"], EXTERNAL_COOKIES);
    } else {
        assert!(!headers.contains_key("authorization"));
        assert!(!headers.contains_key("x-api-key"));
        assert_eq!(
            headers["cookie"],
            if internal {
                "theme=dark; cart=123"
            } else {
                "consent=a,b; prefs={\"x\":1}; sid=1; flag"
            }
        );
    }
}

fn assert_query(uri: &http::Uri, forward: bool, original: &str) {
    assert_eq!(
        uri.query(),
        Some(if forward { original } else { "page=2&keep=%2F" })
    );
}

async fn check_http(trusted: bool, internal: bool, forward: bool) {
    let primary = Upstream::start(Outcome::Status(503)).await;
    let backup = Upstream::start(Outcome::Status(200)).await;
    let mirror = Upstream::start(Outcome::Status(200)).await;
    let mut config =
        serde_json::to_value(super::replay_support::config(&primary, &backup)).unwrap();
    config["http"]["upstreams"]["mirror"] = json!({"targets":[{"url":mirror.url}]});
    config["http"]["services"]["mirror"] = json!({"kind":"load_balancer","upstream":"mirror"});
    config["http"]["services"]["root"] =
        json!({"kind":"mirror","service":"entry","mirrors":[{"service":"mirror","percent":100}]});
    if forward {
        for name in ["primary", "backup"] {
            config["http"]["upstreams"][name]["forward_credentials"] = true.into();
        }
    }
    if !internal {
        // A mirror can have a different policy from its primary and failover.
        config["http"]["upstreams"]["mirror"]["forward_credentials"] = (!forward).into();
    }
    let runtime = runtime(config, internal);
    // HTTP/2 can carry the original authority without a Host header. Replay
    // reconstructs the upstream URI and must retain these ingress facts.
    let mut req = incoming(
        &format!("https://{ORIGINAL_HOST}/orders?{HTTP_QUERY}"),
        internal,
    );
    *req.version_mut() = http::Version::HTTP_2;
    let state = request_state(&runtime, &mut req, internal);
    req.headers_mut()
        .insert(HOST, "rewritten.test".parse().unwrap());
    let plan = build_execution_plan(&runtime.core.graph, "root", "request-id").unwrap();
    let response = execute_plan_with_request(&runtime, &plan, req, &state)
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    drop(response);
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.resources.available().1 != runtime.settings.budgets.mirror_concurrency {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    for (upstream, upstream_forward) in [
        (&primary, forward),
        (&backup, forward),
        (&mirror, !internal && !forward),
    ] {
        let records = upstream.records();
        assert_eq!(records.len(), 1);
        assert_query(&records[0].uri, upstream_forward, HTTP_QUERY);
        assert_headers(
            &records[0].headers,
            trusted,
            internal,
            upstream_forward,
            "HTTP replay",
        );
    }

    // A direct HTTP/1 dispatch takes the Host path and does not use replay.
    let mut req = incoming(&format!("/orders?{HTTP_QUERY}"), internal);
    req.headers_mut()
        .insert(HOST, ORIGINAL_HOST.parse().unwrap());
    let state = request_state(&runtime, &mut req, internal);
    let plan = build_execution_plan(&runtime.core.graph, "backup", "request-id").unwrap();
    let response = execute_plan_with_request(&runtime, &plan, req, &state)
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let records = backup.records();
    assert_eq!(records.len(), 2);
    assert_query(&records[1].uri, forward, HTTP_QUERY);
    assert_headers(
        &records[1].headers,
        trusted,
        internal,
        forward,
        "HTTP direct",
    );
}

// Tungstenite's handshake callback fixes the error type to a full HTTP response.
#[allow(clippy::result_large_err)]
async fn check_websocket(trusted: bool, internal: bool, forward: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}", listener.local_addr().unwrap());
    let (sender, captured) = oneshot::channel();
    let upstream = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let callback = move |request: &WsRequest, response: WsResponse| {
            sender
                .send((request.uri().clone(), request.headers().clone()))
                .unwrap();
            Ok(response)
        };
        let mut socket = accept_hdr_async(stream, callback).await.unwrap();
        let message = socket.next().await.unwrap().unwrap();
        socket.send(message).await.unwrap();
        let _ = socket.next().await;
    });
    let mut config = serde_json::to_value(gate::cfg::Config::default()).unwrap();
    config["http"] = json!({
        "upstreams":{"socket":{"targets":[{"url":upstream_url}]}},
        "services":{"socket":{"kind":"load_balancer","upstream":"socket"}}
    });
    if forward {
        config["http"]["upstreams"]["socket"]["forward_credentials"] = true.into();
    }
    let runtime = runtime(config, internal);
    let app = Router::new().fallback(any(move |mut req: Request<Body>| {
        let runtime = runtime.clone();
        async move {
            let state = request_state(&runtime, &mut req, internal);
            let plan = build_execution_plan(&runtime.core.graph, "socket", "request-id").unwrap();
            execute_plan_with_request(&runtime, &plan, req, &state).await
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/socket?{WS_QUERY}", listener.local_addr().unwrap());
    let gateway = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert(HOST, ORIGINAL_HOST.parse().unwrap());
    add_proxy_headers(request.headers_mut(), internal);
    request.headers_mut().insert(
        http::header::CONNECTION,
        "upgrade, x-connection-secret".parse().unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let (uri, headers) = captured.await.unwrap();
    assert_query(&uri, forward, WS_QUERY);
    assert_headers(&headers, trusted, internal, forward, "WebSocket");
    socket.send(Message::Text("echo".into())).await.unwrap();
    assert_eq!(
        socket.next().await.unwrap().unwrap(),
        Message::Text("echo".into())
    );
    socket.close(None).await.unwrap();
    upstream.await.unwrap();
    gateway.abort();
}

#[cfg(feature = "memory")]
async fn check_gateway_ingress(trusted: bool) {
    let upstream = Upstream::start(Outcome::Status(200)).await;
    let mut config = serde_json::to_value(gate::cfg::Config::default()).unwrap();
    config["http"] = json!({
        "upstreams": {"orders": {"targets": [{"url": upstream.url}]}},
        "services": {
            "orders": {"kind": "load_balancer", "upstream": "orders"},
            "matched": {"kind": "direct_response", "status": 201},
            "fallback": {"kind": "direct_response", "status": 404}
        },
        "middlewares": {
            "strip-api": {"kind": "strip_prefix", "prefixes": ["/api"]},
            "prefix": {"kind": "request_headers", "set": [{"name": "x-forwarded-prefix", "value": "/api"}]}
        },
        "routers": {
            "matched": {"priority": 10, "match": {"all": [
                {"path": {"exact": "/match"}},
                {"header": {"name": "x-forwarded-host", "eq": "proxy.test:443"}}
            ]}, "service": "matched"},
            "proxy": {"match": {"path": {"prefix": "/api"}}, "service": "orders", "middlewares": ["strip-api", "prefix"]},
            "fallback": {"priority": -10, "match": {"path": {"prefix": "/"}}, "service": "fallback"}
        }
    });
    let gate = Arc::new(crate::gate::test_support::gate(
        serde_json::from_value(config).unwrap(),
    ));
    for (path, status) in [
        ("/api/orders", 200),
        ("/match", if trusted { 201 } else { 404 }),
    ] {
        let mut req = Request::builder()
            .uri(path)
            .header(HOST, ORIGINAL_HOST)
            .header("x-forwarded-host", "proxy.test:443")
            .header("x-forwarded-prefix", "/evil")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
        req.extensions_mut().insert(gate.clone());
        let response = crate::gate::service(req).await.unwrap();
        assert_eq!(response.status(), status);
    }
    let records = upstream.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].uri.path(), "/orders");
    assert_eq!(records[0].headers["x-forwarded-prefix"], "/api");
}

#[test]
fn upstreams_share_the_forwarding_trust_boundary() {
    const CASE: &str = "STARGATE_TEST_FORWARDED_PEER";
    if let Ok(case) = std::env::var(CASE) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for internal in [false, true] {
                for forward in [false, true] {
                    if internal && forward {
                        continue;
                    }
                    check_http(case == "trusted", internal, forward).await;
                    check_websocket(case == "trusted", internal, forward).await;
                }
            }
            #[cfg(feature = "memory")]
            check_gateway_ingress(case == "trusted").await;
        });
        return;
    }
    // The proxy list is process-wide and cached. Child processes exercise its
    // production lookup without mutating the environment of concurrent tests.
    for case in ["trusted", "untrusted"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "gate::tests::forwarded_headers::upstreams_share_the_forwarding_trust_boundary",
                "--nocapture",
            ])
            .env(CASE, case)
            .env(
                "TRUSTED_PROXIES",
                if case == "trusted" {
                    "127.0.0.1/32"
                } else {
                    ""
                },
            )
            .env("TLS_ENABLED", "false")
            .env("OAUTH_BASE_URL", "https://public.test")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{case}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
