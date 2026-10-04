use super::{
    execution::execute_plan_from_replay,
    executor::execute_selected_with_request,
    lifecycle::Execution,
    mirrors::{admit_mirrors, spawn_mirrors},
    replay::buffer_request,
    types::{ExecutionPlan, ReplayRequest, RequestState, SelectedService},
};
use crate::{
    err::ErrorCode,
    etc::gate::{RuntimeSnapshot, test_support},
};
use axum::{Router, body::Body, response::Response};
use futures_util::{StreamExt, stream};
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use hyper::body::Bytes;
use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle};

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
    async fn new(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { url, task }
    }
}

fn config(url: &str) -> gate::cfg::Config {
    serde_json::from_value(serde_json::json!({
        "schema": gate::cfg::SCHEMA,
        "ingress": {"limit": "ingress", "timeout": "250ms"},
        "limits": {"ingress": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "100ms"}}},
        "runtime": {
            "primary_concurrency": 2, "mirror_concurrency": 1,
            "replay_body_bytes": 1024, "replay_memory_bytes": 2048,
            "upload_idle_timeout": "100ms", "connect_timeout": "100ms",
            "response_header_timeout": "200ms", "response_body_idle_timeout": "100ms",
            "request_timeout": "2s", "mirror_timeout": "300ms"
        },
        "http": {
            "upstreams": {"leaf": {"targets": [{"url": url}], "load_balancer": {"strategy":"round_robin", "circuit_breaker": {"fail_threshold":1, "cooldown":"3600s"}}}},
            "services": {"leaf": {"kind":"load_balancer", "upstream":"leaf"}},
            "routers": {"leaf": {"match":{"path":{"prefix":"/"}}, "service":"leaf"}}
        }
    })).unwrap()
}
fn runtime(config: gate::cfg::Config) -> Arc<RuntimeSnapshot> {
    test_support::runtime(gate::cfg::RuntimeConfig::from_raw(config).unwrap())
}
fn selected(url: &str) -> SelectedService {
    SelectedService::Upstream {
        service_name: "leaf".into(),
        upstream_base_url: url.into(),
        internal_context: None,
    }
}
fn planned() -> ExecutionPlan {
    ExecutionPlan::Upstream {
        service_name: "leaf".into(),
        internal_context: None,
    }
}
fn state(runtime: &RuntimeSnapshot) -> RequestState {
    RequestState {
        execution: Execution::new(
            runtime.settings.request_timeout,
            runtime.resources.shutdown.child_token(),
        ),
        client_ip: "127.0.0.1".into(),
        load_balancer_key: None,
        original_path: "/orders".into(),
        path: "/orders".into(),
        query: String::new(),
        preserve_host: false,
        response_headers: Default::default(),
        propagation_draft: None,
        internal_context_runtime: None,
    }
}
fn replay() -> ReplayRequest {
    ReplayRequest {
        method: http::Method::GET,
        version: http::Version::HTTP_11,
        headers: http::HeaderMap::new(),
        body: Bytes::new(),
    }
}
fn healthy(runtime: &RuntimeSnapshot) -> bool {
    runtime.core.balancers["leaf"]
        .select(&lb::RequestContext {
            client_ip: "127.0.0.1",
            path: "/",
            method: "GET",
            key: None,
        })
        .is_some()
}

#[tokio::test]
async fn missing_headers_and_stalled_upload_are_distinct_local_timeouts() {
    let upstream = Upstream::new(
        Router::new().fallback(|| async { std::future::pending::<Response>().await }),
    )
    .await;
    let runtime = runtime(config(&upstream.url));
    let error = execute_selected_with_request(
        &runtime,
        &selected(&upstream.url),
        Request::new(Body::empty()),
        &state(&runtime),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::GatewayTimeout);
    assert_eq!(error.params["phase"], "response_headers");
    assert!(healthy(&runtime));

    let upstream = Upstream::new(Router::new().fallback(|request: Request<Body>| async move {
        let _ = request.into_body().collect().await;
        "done"
    }))
    .await;
    let runtime = self::runtime(config(&upstream.url));
    let request = Request::builder()
        .method("POST")
        .uri("/orders")
        .body(Body::from_stream(stream::pending::<
            Result<Bytes, Infallible>,
        >()))
        .unwrap();
    let error = execute_selected_with_request(
        &runtime,
        &selected(&upstream.url),
        request,
        &state(&runtime),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::GatewayUploadTimeout);
    assert!(healthy(&runtime));
}

#[tokio::test]
async fn tls_connection_and_websocket_handshake_stalls_expire() {
    for websocket in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let url = format!(
            "{}://localhost:{}",
            if websocket { "http" } else { "https" },
            address.port()
        );
        let runtime = runtime(config(&url));
        let mut request = Request::new(Body::empty());
        if websocket {
            request
                .headers_mut()
                .insert("connection", "Upgrade".parse().unwrap());
            request
                .headers_mut()
                .insert("upgrade", "websocket".parse().unwrap());
            request
                .headers_mut()
                .insert("sec-websocket-version", "13".parse().unwrap());
            request.headers_mut().insert(
                "sec-websocket-key",
                "dGhlIHNhbXBsZSBub25jZQ==".parse().unwrap(),
            );
        }
        let error =
            execute_selected_with_request(&runtime, &selected(&url), request, &state(&runtime))
                .await
                .unwrap_err();
        assert_eq!(error.code, ErrorCode::GatewayTimeout);
        assert_eq!(
            error.params["phase"],
            if websocket {
                "websocket_handshake"
            } else {
                "connect"
            }
        );
        assert!(healthy(&runtime));
        server.abort();
    }
}

#[tokio::test]
async fn response_body_idle_timeout_never_starts_failover() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let upstream = Upstream::new(Router::new().fallback(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        async {
            Response::new(Body::from_stream(
                stream::once(async { Ok::<_, Infallible>(Bytes::from_static(b"first")) })
                    .chain(stream::pending()),
            ))
        }
    }))
    .await;
    let runtime = runtime(config(&upstream.url));
    let plan = ExecutionPlan::Failover {
        services: vec![planned(); 2],
        on_status: vec![503],
    };
    let mut response = execute_plan_from_replay(
        &runtime,
        &plan,
        &replay(),
        &state(&runtime),
        ctx::DispatchKind::Primary,
    )
    .await
    .unwrap();
    assert_eq!(
        response
            .body_mut()
            .frame()
            .await
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap(),
        "first"
    );
    assert!(response.into_body().collect().await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failover_chain_does_not_reset_total_deadline() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let upstream = Upstream::new(Router::new().fallback(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        async {
            tokio::time::sleep(Duration::from_millis(150)).await;
            StatusCode::SERVICE_UNAVAILABLE
        }
    }))
    .await;
    let mut raw = config(&upstream.url);
    raw.runtime.request_timeout = "250ms".into();
    raw.http
        .upstreams
        .get_mut("leaf")
        .unwrap()
        .load_balancer
        .circuit_breaker
        .as_mut()
        .unwrap()
        .fail_threshold = Some(3);
    let runtime = runtime(raw);
    let plan = ExecutionPlan::Failover {
        services: vec![planned(); 3],
        on_status: vec![503],
    };
    let error = execute_plan_from_replay(
        &runtime,
        &plan,
        &replay(),
        &state(&runtime),
        ctx::DispatchKind::Primary,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::GatewayTimeout);
    assert_eq!(error.params["phase"], "total");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn mirror_saturation_is_bounded_and_shutdown_releases_replay_storage() {
    let upstream = Upstream::new(
        Router::new().fallback(|| async { std::future::pending::<Response>().await }),
    )
    .await;
    let runtime = runtime(config(&upstream.url));
    let state = state(&runtime);
    let replay = buffer_request(
        Request::new(Body::from("payload")),
        &runtime,
        &state.execution,
        runtime
            .resources
            .reserve_replay(runtime.settings.replay_body_bytes)
            .unwrap(),
    )
    .await
    .unwrap();
    let plan = planned();
    spawn_mirrors(
        runtime.clone(),
        admit_mirrors(&runtime, &vec![plan; 100]),
        replay,
        state.clone(),
    );
    assert_eq!(runtime.resources.available(), (2, 0, 1024));
    let primary = SelectedService::DirectResponse {
        status: 200,
        headers: Vec::new(),
        body: None,
    };
    assert_eq!(
        execute_selected_with_request(&runtime, &primary, Request::new(Body::empty()), &state)
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    runtime.resources.begin_shutdown();
    tokio::time::timeout(Duration::from_secs(1), runtime.resources.wait())
        .await
        .unwrap();
    assert_eq!(runtime.resources.available(), (2, 1, 2048));
}

#[cfg(feature = "memory")]
#[tokio::test]
async fn ingress_overload_and_body_drop_keep_budgets_shared_across_reload() {
    let upstream = Upstream::new(Router::new().fallback(|| async {
        Response::new(Body::from_stream(stream::pending::<
            Result<Bytes, Infallible>,
        >()))
    }))
    .await;
    let raw = config(&upstream.url);
    let gate = Arc::new(test_support::gate(raw.clone()));
    let request = || {
        let mut request = Request::new(Body::empty());
        request.extensions_mut().insert(gate.clone());
        request.extensions_mut().insert(axum::extract::ConnectInfo(
            "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
        ));
        request
    };
    let first = super::handle_hyper(request()).await.unwrap();
    let old = gate.snapshot();
    let mut raw = raw;
    raw.runtime.response_body_idle_timeout = "150ms".into();
    gate.activate(crate::etc::gate::prepare_config(raw.clone()).unwrap())
        .unwrap();
    assert!(Arc::ptr_eq(&old.resources, &gate.snapshot().resources));
    let second = super::handle_hyper(request()).await.unwrap();
    let error = super::handle_hyper(request()).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::GatewayOverloaded);
    drop(first);
    assert_eq!(gate.resources.available().0, 1);
    drop(second);
    assert_eq!(gate.resources.available().0, 2);
    raw.runtime.primary_concurrency = 3;
    assert!(matches!(
        gate.activate(crate::etc::gate::prepare_config(raw).unwrap()),
        Err(crate::etc::gate::GatewayPreparationError::BudgetChange)
    ));
    assert_eq!(gate.snapshot().version, 1);
}

#[cfg(feature = "memory")]
fn ingress_request(gate: &Arc<crate::etc::gate::Gate>) -> Request<Body> {
    let mut request = Request::new(Body::empty());
    request.extensions_mut().insert(gate.clone());
    request.extensions_mut().insert(axum::extract::ConnectInfo(
        "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
    ));
    request
}

#[cfg(feature = "memory")]
#[tokio::test]
async fn stream_route_survives_finite_budget_and_finite_route_expires() {
    let upstream = Upstream::new(Router::new().fallback(|| async {
        Response::new(Body::from_stream(stream::unfold(0, |index| async move {
            if index == 8 {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
            Some((Ok::<_, Infallible>(Bytes::from_static(b"data")), index + 1))
        })))
    }))
    .await;
    let mut raw = config(&upstream.url);
    raw.runtime.request_timeout = "150ms".into();
    let gate = Arc::new(test_support::gate(raw.clone()));
    let response = super::handle_hyper(ingress_request(&gate)).await.unwrap();
    assert!(response.into_body().collect().await.is_err());
    assert_eq!(gate.resources.available().0, 2);
    raw.http.routers.get_mut("leaf").unwrap().response_mode = gate::cfg::ResponseMode::Stream;
    gate.activate(crate::etc::gate::prepare_config(raw).unwrap())
        .unwrap();
    let response = super::handle_hyper(ingress_request(&gate)).await.unwrap();
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .len(),
        32
    );
    assert_eq!(gate.resources.available().0, 2);
}

#[cfg(feature = "memory")]
#[tokio::test]
async fn cancellation_before_headers_releases_primary_and_replay_permits() {
    let contacted = Arc::new(tokio::sync::Notify::new());
    let notify = contacted.clone();
    let upstream = Upstream::new(Router::new().fallback(move || {
        notify.notify_one();
        async { std::future::pending::<Response>().await }
    }))
    .await;
    let mut raw = config(&upstream.url);
    raw.http.services.insert(
        "failover".into(),
        gate::cfg::Service::Failover {
            service: "leaf".into(),
            failovers: vec!["leaf".into()],
            on_status: Vec::new(),
        },
    );
    raw.http.routers.get_mut("leaf").unwrap().service = "failover".into();
    let gate = Arc::new(test_support::gate(raw));
    let request = ingress_request(&gate);
    let task = tokio::spawn(super::handle_hyper(request));
    contacted.notified().await;
    assert_eq!(gate.resources.available(), (1, 1, 1024));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    // Hyper's outbound body ownership may be released asynchronously.
    tokio::time::timeout(Duration::from_secs(1), async {
        while gate.resources.available() != (2, 1, 2048) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[cfg(feature = "memory")]
#[tokio::test]
async fn websocket_session_holds_admission_until_idle_disconnect_or_shutdown() {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    let upstream = Upstream::new(
        Router::new().fallback(|mut request: Request<Body>| async move {
            let (response, websocket) = hyper_tungstenite::upgrade(&mut request, None).unwrap();
            tokio::spawn(async move {
                let mut socket = websocket.await.unwrap();
                while let Some(Ok(message)) = socket.next().await {
                    if socket.send(message).await.is_err() {
                        break;
                    }
                }
            });
            response.map(Body::new)
        }),
    )
    .await;
    let mut raw = config(&upstream.url);
    raw.runtime.request_timeout = "150ms".into();
    let gate = Arc::new(test_support::gate(raw));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/socket", listener.local_addr().unwrap());
    let app = Router::new()
        .fallback(super::service)
        .layer(axum::Extension(gate.clone()));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let (mut client, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    assert_eq!(gate.resources.available().0, 1);
    for _ in 0..6 {
        tokio::time::sleep(Duration::from_millis(40)).await;
        client.send(Message::Text("echo".into())).await.unwrap();
        assert_eq!(
            client.next().await.unwrap().unwrap(),
            Message::Text("echo".into())
        );
    }
    assert_eq!(gate.resources.available().0, 1);
    let _ = tokio::time::timeout(Duration::from_secs(1), client.next())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while gate.resources.available().0 != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let (mut client, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    gate.resources.begin_shutdown();
    tokio::time::timeout(Duration::from_secs(1), gate.resources.wait())
        .await
        .unwrap();
    assert_eq!(gate.resources.available().0, 2);
    let _ = tokio::time::timeout(Duration::from_secs(1), client.next())
        .await
        .unwrap();
    server.abort();
}

#[cfg(feature = "memory")]
#[tokio::test]
async fn mirror_capacity_and_memory_exhaustion_preserve_primary_streaming() {
    let upstream = Upstream::new(Router::new().fallback(|request: Request<Body>| async move {
        request.into_body().collect().await.unwrap().to_bytes()
    }))
    .await;
    let mut raw = config(&upstream.url);
    raw.http.services.insert(
        "mirror".into(),
        gate::cfg::Service::Mirror {
            service: "leaf".into(),
            mirrors: vec![
                serde_json::from_value(serde_json::json!({"service":"leaf", "percent":100}))
                    .unwrap(),
            ],
        },
    );
    raw.http.routers.get_mut("leaf").unwrap().service = "mirror".into();
    let gate = Arc::new(test_support::gate(raw));
    for saturate_memory in [false, true] {
        let permit = if saturate_memory {
            gate.resources.reserve_replay(2048).unwrap()
        } else {
            gate.resources.admit_mirror().unwrap()
        };
        let mut request = ingress_request(&gate);
        *request.method_mut() = http::Method::POST;
        // Exceeds the replay cap and omits Content-Length: primary must stream it.
        *request.body_mut() = Body::from(Bytes::from(vec![b'x'; 4096]));
        let response = super::handle_hyper(request).await.unwrap();
        assert_eq!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .len(),
            4096
        );
        drop(permit);
        assert_eq!(gate.resources.available(), (2, 1, 2048));
    }
}

#[tokio::test]
async fn progressing_mirror_still_expires_at_its_total_deadline() {
    let upstream = Upstream::new(Router::new().fallback(|| async {
        Response::new(Body::from_stream(stream::unfold((), |_| async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Some((Ok::<_, Infallible>(Bytes::from_static(b"progress")), ()))
        })))
    }))
    .await;
    let mut raw = config(&upstream.url);
    raw.runtime.mirror_timeout = "100ms".into();
    let runtime = runtime(raw);
    let state = state(&runtime);
    let replay = buffer_request(
        Request::new(Body::from("payload")),
        &runtime,
        &state.execution,
        runtime.resources.reserve_replay(1024).unwrap(),
    )
    .await
    .unwrap();
    let plan = planned();
    spawn_mirrors(
        runtime.clone(),
        admit_mirrors(&runtime, &[plan]),
        replay,
        state,
    );
    assert_eq!(runtime.resources.available(), (2, 0, 1024));
    tokio::time::timeout(Duration::from_millis(500), async {
        while runtime.resources.available() != (2, 1, 2048) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(healthy(&runtime));
    runtime.resources.begin_shutdown();
    runtime.resources.wait().await;
}
