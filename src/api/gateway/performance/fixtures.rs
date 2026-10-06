use crate::etc::{
    gate::{Gate, test_support},
    observability::logging::trace_middleware,
};
use axum::{
    Extension, Router, body::Body, middleware::from_fn, response::Response, routing::get,
    serve::ListenerExt,
};
use futures_util::{SinkExt, StreamExt, stream};
use gate::cfg::Config;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use hyper::body::Bytes;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use serde_json::json;
use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scenario {
    Plain,
    Streaming,
    Authenticated,
    Signing,
    MixedSigning,
    Mirror,
    SlowMirror,
    StatusFailover,
    TransportFailover,
    SlowFailure,
    Reload,
    LiveStreams,
    Admission,
    ReplaySaturation,
}
impl Scenario {
    pub(super) const ALL: [Self; 14] = [
        Self::Plain,
        Self::Streaming,
        Self::Authenticated,
        Self::Signing,
        Self::MixedSigning,
        Self::Mirror,
        Self::SlowMirror,
        Self::StatusFailover,
        Self::TransportFailover,
        Self::SlowFailure,
        Self::Reload,
        Self::LiveStreams,
        Self::Admission,
        Self::ReplaySaturation,
    ];
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Streaming => "streaming",
            Self::Authenticated => "authenticated_hs256",
            Self::Signing => "context_signing",
            Self::MixedSigning => "mixed_signing_short",
            Self::Mirror => "mirror",
            Self::SlowMirror => "slow_mirror",
            Self::StatusFailover => "status_failover",
            Self::TransportFailover => "transport_failover",
            Self::SlowFailure => "slow_failure",
            Self::Reload => "reload",
            Self::LiveStreams => "live_streams_websockets_short",
            Self::Admission => "primary_saturation",
            Self::ReplaySaturation => "replay_saturation",
        }
    }
    pub(super) fn path(self) -> &'static str {
        match self {
            Self::Plain | Self::Reload => "/plain",
            Self::Streaming => "/stream",
            Self::Authenticated => "/auth",
            Self::Signing | Self::MixedSigning => "/sign",
            Self::Mirror => "/mirror",
            Self::SlowMirror => "/slow-mirror",
            Self::StatusFailover => "/status",
            Self::TransportFailover => "/transport",
            Self::SlowFailure | Self::ReplaySaturation => "/slow-failure",
            Self::LiveStreams => "/short",
            Self::Admission => "/slow",
        }
    }
    pub(super) fn auth(self) -> bool {
        matches!(
            self,
            Self::Authenticated | Self::Signing | Self::MixedSigning
        )
    }
    pub(super) fn concurrency(self) -> usize {
        16
    }
    pub(super) fn body_bytes(self) -> usize {
        if matches!(self, Self::SlowMirror | Self::ReplaySaturation) {
            64 * 1024
        } else {
            0
        }
    }
}

struct Server {
    url: String,
    task: JoinHandle<()>,
    connections: Arc<AtomicUsize>,
}
impl Server {
    async fn start(app: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let connections = Arc::new(AtomicUsize::new(0));
        let count = connections.clone();
        let listener = listener.tap_io(move |_| {
            count.fetch_add(1, Ordering::Relaxed);
        });
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Self {
            url,
            task,
            connections,
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn streaming(frames: usize, interval: Duration, status: StatusCode) -> Response {
    let body = stream::unfold(0_usize, move |index| async move {
        if index >= frames {
            return None;
        }
        tokio::time::sleep(interval).await;
        Some((
            Ok::<_, Infallible>(Bytes::from_static(&[b'x'; 1024])),
            index + 1,
        ))
    });
    let mut response = Response::new(Body::from_stream(body));
    *response.status_mut() = status;
    response
}

async fn websocket(mut request: Request<Body>) -> Response {
    let (response, upgrade) = hyper_tungstenite::upgrade(&mut request, None).unwrap();
    tokio::spawn(async move {
        let mut socket = upgrade.await.unwrap();
        while let Some(Ok(message)) = socket.next().await {
            if message.is_close() {
                let _ = socket.close(None).await;
                break;
            }
            if socket.send(message).await.is_err() {
                break;
            }
        }
    });
    response.map(Body::new)
}

#[derive(Clone)]
pub(super) struct Fixture {
    pub(super) gate: Arc<Gate>,
    pub(super) config: Config,
    server: Arc<Server>,
    upstream: Arc<Server>,
    broken: Arc<Server>,
    client: Client<HttpConnector, Body>,
}

impl Fixture {
    pub(super) async fn new(scenario: Scenario) -> Self {
        let upstream = Arc::new(
            Server::start(
                Router::new()
                    .route(
                        "/stream",
                        get(|| async { streaming(3, Duration::from_millis(1), StatusCode::OK) }),
                    )
                    .route(
                        "/live",
                        get(|| async {
                            streaming(usize::MAX, Duration::from_millis(20), StatusCode::OK)
                        }),
                    )
                    .route("/socket", get(websocket))
                    .route(
                        "/stall",
                        get(|| async {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            Response::new(Body::empty())
                        }),
                    )
                    .route(
                        "/stall/{*rest}",
                        get(|| async {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            Response::new(Body::empty())
                        }),
                    )
                    .route(
                        "/slow",
                        get(|| async {
                            tokio::time::sleep(Duration::from_millis(20)).await;
                            "ready"
                        }),
                    )
                    .route(
                        "/failure/{*rest}",
                        get(|request: Request<Body>| async move {
                            if request.uri().path().contains("slow") {
                                tokio::time::sleep(Duration::from_millis(20)).await;
                            }
                            streaming(1024 * 64, Duration::ZERO, StatusCode::SERVICE_UNAVAILABLE)
                        }),
                    )
                    .fallback(|request: Request<Body>| async move {
                        request.into_body().collect().await.unwrap();
                        Bytes::from_static(&[b'x'; 1024])
                    }),
            )
            .await,
        );
        // A local endpoint that accepts TCP and immediately closes each connection.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let connections = Arc::new(AtomicUsize::new(0));
        let count = connections.clone();
        let task = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                count.fetch_add(1, Ordering::Relaxed);
                drop(socket);
            }
        });
        let broken = Arc::new(Server {
            url,
            task,
            connections,
        });
        let mut value = serde_json::to_value(Config::default()).unwrap();
        for name in ["default", "ingress"] {
            value["limits"][name] = json!({"strategy":"token_bucket", "params":{"capacity":1_000_000, "refill_rate":1_000_000}});
        }
        value["runtime"]["primary_concurrency"] = json!(if scenario == Scenario::Admission {
            2
        } else {
            32
        });
        value["runtime"]["mirror_concurrency"] = json!(4);
        value["runtime"]["replay_body_bytes"] = json!(64 * 1024);
        value["runtime"]["replay_memory_bytes"] =
            json!(if scenario == Scenario::ReplaySaturation {
                128 * 1024
            } else {
                1024 * 1024
            });
        value["runtime"]["response_header_timeout"] = json!("100ms");
        value["runtime"]["discarded_body_timeout"] = json!("20ms");
        value["runtime"]["mirror_timeout"] = json!("200ms");
        value["runtime"]["response_body_idle_timeout"] = json!("1s");
        for name in [
            "plain",
            "signed",
            "shadow",
            "slow-shadow",
            "failure",
            "broken",
        ] {
            let target = if name == "broken" {
                broken.url.clone()
            } else if name == "slow-shadow" {
                format!("{}/stall", upstream.url)
            } else if name == "shadow" {
                format!("{}/shadow", upstream.url)
            } else if name == "failure" {
                format!("{}/failure", upstream.url)
            } else {
                upstream.url.clone()
            };
            value["http"]["upstreams"][name] = json!({"targets":[{"url":target}], "load_balancer":{"strategy":"round_robin", "circuit_breaker":{"fail_threshold":1_000_000}}});
            value["http"]["services"][name] = json!({"kind":"load_balancer", "upstream":name});
        }
        value["http"]["upstreams"]["signed"]["internal_context"] =
            json!({"audience":"urn:stargate:service:benchmark"});
        value["http"]["services"]["short"] =
            json!({"kind":"direct_response", "status":200, "body":{"text":"ready"}});
        for (name, mirror) in [("mirror", "shadow"), ("slow-mirror", "slow-shadow")] {
            value["http"]["services"][name] = json!({"kind":"mirror", "service":"plain", "mirrors":[{"service":mirror, "percent":100}]});
        }
        for (name, first) in [
            ("status", "failure"),
            ("slow-failure", "failure"),
            ("transport", "broken"),
        ] {
            value["http"]["services"][name] = json!({"kind":"failover", "service":first, "failovers":["plain"], "on_status":[503]});
        }
        value["http"]["policies"]["auth"] = json!({"kind":"auth", "strategies":["jwt"]});
        for (path, service) in [
            ("plain", "plain"),
            ("stream", "plain"),
            ("auth", "plain"),
            ("sign", "signed"),
            ("mirror", "mirror"),
            ("slow-mirror", "slow-mirror"),
            ("status", "status"),
            ("transport", "transport"),
            ("slow-failure", "slow-failure"),
            ("short", "short"),
            ("live", "plain"),
            ("socket", "plain"),
            ("slow", "plain"),
        ] {
            value["http"]["routers"][path] = json!({"match":{"path":{"exact":format!("/{path}")}}, "service":service, "response_mode":if matches!(path,"stream"|"live") {"stream"} else {"finite"}, "policies":if matches!(path,"auth"|"sign") {vec!["auth"]} else {vec![]}});
        }
        let config: Config = serde_json::from_value(value).unwrap();
        let gate = Arc::new(test_support::gate(config.clone()));
        let server = Arc::new(
            Server::start(
                Router::new()
                    .fallback(crate::api::gateway::service)
                    .layer(from_fn(trace_middleware))
                    .layer(Extension(gate.clone())),
            )
            .await,
        );
        let client = Client::builder(TokioExecutor::new()).build(HttpConnector::new());
        Self {
            gate,
            config,
            server,
            upstream,
            broken,
            client,
        }
    }

    pub(super) async fn request(
        &self,
        path: &str,
        token: Option<&str>,
        bytes: usize,
    ) -> (u16, f64) {
        let mut request = Request::builder().uri(format!("{}{path}", self.server.url));
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let request = request
            .body(if bytes == 0 {
                Body::empty()
            } else {
                Body::from(vec![b'x'; bytes])
            })
            .unwrap();
        let started = Instant::now();
        let response = match self.client.request(request).await {
            Ok(response) => response,
            Err(_) => return (0, started.elapsed().as_secs_f64() * 1000.0),
        };
        let header_ms = started.elapsed().as_secs_f64() * 1000.0;
        let status = response.status().as_u16();
        if response.into_body().collect().await.is_err() {
            return (0, header_ms);
        }
        (status, header_ms)
    }

    pub(super) async fn background(&self, scenario: Scenario) -> Vec<JoinHandle<()>> {
        if !matches!(scenario, Scenario::LiveStreams | Scenario::MixedSigning) {
            return Vec::new();
        }
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let response = self
                .client
                .request(
                    Request::builder()
                        .uri(format!("{}/live", self.server.url))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            tasks.push(tokio::spawn(async move {
                let mut body = response.into_body();
                while let Some(frame) = body.frame().await {
                    let Ok(frame) = frame else { break };
                    assert!(!frame.into_data().unwrap().is_empty());
                }
            }));
            let (mut socket, response) = tokio_tungstenite::connect_async(format!(
                "{}/socket",
                self.server.url.replacen("http:", "ws:", 1)
            ))
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
            tasks.push(tokio::spawn(async move {
                loop {
                    if socket.send(Message::Text("echo".into())).await.is_err() {
                        break;
                    }
                    match socket.next().await {
                        Some(Ok(message)) if !message.is_close() => {
                            assert_eq!(message, Message::Text("echo".into()))
                        }
                        _ => break,
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }));
        }
        tasks
    }
    pub(super) fn connection_counts(&self) -> serde_json::Value {
        json!({"upstream":self.upstream.connections.load(Ordering::Relaxed), "broken":self.broken.connections.load(Ordering::Relaxed), "version":self.gate.snapshot().version})
    }
    pub(super) async fn stop(self) {
        println!(
            "GATEWAY_CONNECTIONS {}",
            json!({"upstream":self.upstream.connections.load(Ordering::Relaxed), "broken":self.broken.connections.load(Ordering::Relaxed), "version":self.gate.snapshot().version})
        );
        self.gate.resources.begin_shutdown();
        tokio::time::timeout(Duration::from_secs(1), self.gate.resources.wait())
            .await
            .unwrap();
    }
}
