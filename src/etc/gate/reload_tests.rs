use super::test_support::{CA, CLIENT_KEY, SERVER_CERT, SERVER_KEY, TlsFiles};
use super::{
    Gate, GatewayPreparationError, RuntimeSnapshot, load_config_from_path,
    reload_gateway_config_from_path, transport::TransportPreparationError,
};
use axum::body::Body;
use gate::PolicySnapshot;
use http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use hyper::{server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use std::{
    convert::Infallible,
    io::BufReader,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};

struct MtlsUpstream {
    url: String,
    task: JoinHandle<()>,
    paths: Arc<Mutex<Vec<String>>>,
}

impl MtlsUpstream {
    async fn start() -> Self {
        Self::generation(true, "mtls ready").await
    }

    async fn generation(use_tls: bool, label: &'static str) -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_pemfile::certs(&mut BufReader::new(CA)) {
            roots.add(cert.unwrap()).unwrap();
        }
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots),
            provider.clone(),
        )
        .build()
        .unwrap();
        let certificates = rustls_pemfile::certs(&mut BufReader::new(SERVER_CERT))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let key = rustls_pemfile::private_key(&mut BufReader::new(SERVER_KEY))
            .unwrap()
            .unwrap();
        let tls = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(certificates, key)
            .unwrap();
        let acceptor = use_tls.then(|| tokio_rustls::TlsAcceptor::from(Arc::new(tls)));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "{}://{}",
            if use_tls { "https" } else { "http" },
            listener.local_addr().unwrap()
        );
        let paths = Arc::new(Mutex::new(Vec::new()));
        let recorded = paths.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (socket, _) = accepted.unwrap();
                        let acceptor = acceptor.clone();
                        let paths = recorded.clone();
                        connections.spawn(async move {
                            if let Some(acceptor) = acceptor {
                                serve(acceptor.accept(socket).await.unwrap(), label, paths).await;
                            } else {
                                serve(socket, label, paths).await;
                            }
                        });
                    }
                    Some(result) = connections.join_next(), if !connections.is_empty() => {
                        result.unwrap();
                    }
                }
            }
        });
        Self { url, task, paths }
    }
}

async fn serve<I>(io: I, label: &'static str, paths: Arc<Mutex<Vec<String>>>)
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |req: Request<hyper::body::Incoming>| {
        let path = req.uri().path().to_string();
        paths.lock().unwrap().push(path.clone());
        async move {
            let (status, body) = if path.starts_with("/primary") {
                (503, format!("{label} primary"))
            } else if path.starts_with("/backup") {
                (200, format!("{label} backup"))
            } else if path.starts_with("/mirror") {
                (200, format!("{label} mirror"))
            } else {
                (200, label.to_string())
            };
            // Every request must complete a fresh (mutually authenticated when
            // TLS is enabled) handshake, even after credential files disappear.
            Ok::<_, Infallible>(
                Response::builder()
                    .status(status)
                    .header("connection", "close")
                    .body(Body::from(body))
                    .unwrap(),
            )
        }
    });
    http1::Builder::new()
        .serve_connection(TokioIo::new(io), service)
        .await
        .unwrap();
}

impl Drop for MtlsUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn assert_upstream_works(runtime: &RuntimeSnapshot, url: &str) {
    let client = runtime.client("secure").unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        let response = client
            .request(Request::builder().uri(url).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "mtls ready"
        );
    })
    .await
    .expect("mTLS probe timed out");
}

#[tokio::test]
async fn failed_tls_reload_keeps_routes_and_prepared_clients_then_recovers() {
    let upstream = MtlsUpstream::start().await;
    let files = TlsFiles::new();
    let path = files.path("config.yaml");
    let mut raw = files.config(&upstream.url);
    raw.to_file(&path);
    let prepared = load_config_from_path(&path).unwrap();
    let gate = Gate::new(
        Arc::new(lim::State::new()),
        prepared,
        PolicySnapshot::default(),
    )
    .unwrap();
    let runtime = gate.snapshot();
    assert_upstream_works(&runtime, &upstream.url).await;

    raw.http.routers.get_mut("secure").unwrap().priority = Some(123);
    raw.to_file(&path);
    std::fs::remove_file(files.path("client-key.pem")).unwrap();
    let error = reload_gateway_config_from_path(&gate, &path).unwrap_err();
    assert!(matches!(
        error,
        GatewayPreparationError::Transport(TransportPreparationError::ReadFile {
            kind: "client key",
            ..
        })
    ));
    assert!(!gate.activation.is_poisoned());
    assert!(Arc::ptr_eq(&runtime, &gate.snapshot()));
    assert_eq!(gate.snapshot().version, 0);
    assert_eq!(gate.snapshot().core.graph.routers[0].priority, 0);
    assert_upstream_works(&runtime, &upstream.url).await;

    files.write("client-key.pem", CLIENT_KEY);
    assert_eq!(reload_gateway_config_from_path(&gate, &path).unwrap(), 1);
    assert!(!Arc::ptr_eq(&runtime, &gate.snapshot()));
    assert_eq!(gate.snapshot().core.graph.routers[0].priority, 123);
    assert_upstream_works(&gate.snapshot(), &upstream.url).await;
}

#[test]
fn invalid_initial_transport_returns_a_preparation_error() {
    let files = TlsFiles::new();
    let path = files.path("config.yaml");
    files.config("https://localhost:8443").to_file(&path);
    files.write("ca.pem", b"");
    assert!(matches!(
        load_config_from_path(&path),
        Err(GatewayPreparationError::Transport(
            TransportPreparationError::EmptyCertificates { kind: "CA" }
        ))
    ));
}

fn generation_config(
    url: &str,
    name: &str,
    mtls: Option<gate::cfg::MtlsConfig>,
) -> gate::cfg::Config {
    let primary = format!("{name}-primary");
    let backup = format!("{name}-backup");
    let mirror = format!("{name}-mirror");
    let failover = format!("{name}-failover");
    let entry = format!("{name}-entry");
    let limit = format!("{name}-rate");
    serde_json::from_value(serde_json::json!({
        "schema": gate::cfg::SCHEMA,
        "ingress": {"limit": "ingress", "timeout": "250ms"},
        "mtls": mtls,
        "limits": {"default": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "1s"}}, "ingress": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "100ms"}}, limit.clone(): {"strategy": "token_bucket", "params": {"capacity": if name == "old" {11} else {22}, "refill_rate": 1}}},
        "http": {
            "upstreams": {
                primary.clone(): {"targets": [{"url": format!("{url}/primary")}], "load_balancer": {"strategy": "round_robin", "circuit_breaker": {"fail_threshold": 1, "cooldown": "3600s"}}},
                backup.clone(): {"targets": [{"url": format!("{url}/backup")}]},
                mirror.clone(): {"targets": [{"url": format!("{url}/mirror")}]},
            },
            "services": {
                primary.clone(): {"kind": "load_balancer", "upstream": primary.clone()},
                backup.clone(): {"kind": "load_balancer", "upstream": backup.clone()},
                mirror.clone(): {"kind": "load_balancer", "upstream": mirror.clone()},
                failover.clone(): {"kind": "failover", "service": primary, "failovers": [backup], "on_status": [503]},
                entry.clone(): {"kind": "mirror", "service": failover, "mirrors": [{"service": mirror, "percent": 100}]},
            },
            "policies": {"rate": {"kind": "rate_limit", "limit": limit}},
            "routers": {name: {"match": {"path": {"prefix": "/"}}, "service": entry, "policies": ["rate"]}},
        },
    })).unwrap()
}

fn request(gate: &Arc<Gate>) -> Request<Body> {
    let mut req = Request::builder()
        .uri("/orders")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(gate.clone());
    req
}

async fn wait_for_path(upstream: &MtlsUpstream, expected: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if upstream
                .paths
                .lock()
                .unwrap()
                .iter()
                .any(|path| path == expected)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("upstream never received the expected request");
}

#[tokio::test]
async fn ingress_pin_survives_changed_names_limits_urls_and_tls_through_failover_and_mirror() {
    use crate::api::gateway::IngressPause;
    use tokio::sync::Barrier;

    let old = MtlsUpstream::generation(true, "old").await;
    let new = MtlsUpstream::generation(false, "new").await;
    let files = TlsFiles::new();
    let gate = Arc::new(super::test_support::gate(generation_config(
        &old.url,
        "old",
        Some(files.mtls()),
    )));
    let pinned = gate.snapshot();
    let pause = IngressPause {
        pinned: Arc::new(Barrier::new(2)),
        resume: Arc::new(Barrier::new(2)),
    };
    let mut req = request(&gate);
    req.extensions_mut().insert(pause.clone());
    let old_request = tokio::spawn(crate::api::gateway::service(req));
    pause.pinned.wait().await;

    let version = gate
        .activate(super::prepare_config(generation_config(&new.url, "new", None)).unwrap())
        .unwrap();
    assert_eq!(version, 1);
    assert!(!Arc::ptr_eq(&pinned, &gate.snapshot()));
    assert!(gate.snapshot().client("old-primary").is_none());
    assert!(!gate.snapshot().core.limiter.has_limit("old-rate"));
    std::fs::remove_file(files.path("client-key.pem")).unwrap();
    pause.resume.wait().await;

    let response = tokio::time::timeout(Duration::from_secs(5), old_request)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["x-ratelimit-limit"], "11");
    let context = lb::RequestContext {
        client_ip: "127.0.0.1",
        path: "/orders",
        method: "GET",
        key: None,
    };
    assert!(
        pinned.core.balancers["old-primary"]
            .select(&context)
            .is_none()
    );
    assert!(
        gate.snapshot().core.balancers["new-primary"]
            .select(&context)
            .is_some()
    );
    let body = response.into_body();
    let retained = Arc::downgrade(&pinned);
    drop(pinned);
    assert!(
        retained.upgrade().is_some(),
        "streaming response must retain its generation"
    );
    assert_eq!(body.collect().await.unwrap().to_bytes(), "old backup");
    wait_for_path(&old, "/mirror/orders").await;
    assert!(new.paths.lock().unwrap().is_empty());

    let response = crate::api::gateway::service(request(&gate)).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["x-ratelimit-limit"], "22");
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "new backup"
    );
    wait_for_path(&new, "/mirror/orders").await;
    for upstream in [&old, &new] {
        let paths = upstream.paths.lock().unwrap();
        for expected in ["/primary/orders", "/backup/orders", "/mirror/orders"] {
            assert_eq!(
                paths
                    .iter()
                    .filter(|path| path.as_str() == expected)
                    .count(),
                1
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_activation_and_requests_always_use_a_complete_generation() {
    let old = MtlsUpstream::generation(false, "old").await;
    let new = MtlsUpstream::generation(false, "new").await;
    let gate = Arc::new(super::test_support::gate(generation_config(
        &old.url, "old", None,
    )));
    let mut tasks = JoinSet::new();
    for writer in 0..2 {
        let gate = gate.clone();
        let url = if writer == 0 {
            old.url.clone()
        } else {
            new.url.clone()
        };
        tasks.spawn(async move {
            for _ in 0..16 {
                gate.activate(
                    super::prepare_config(generation_config(
                        &url,
                        if writer == 0 { "old" } else { "new" },
                        None,
                    ))
                    .unwrap(),
                )
                .unwrap();
                tokio::task::yield_now().await;
            }
        });
    }
    for reader in 0..4 {
        let gate = gate.clone();
        tasks.spawn(async move {
            for iteration in 0..16 {
                let runtime = gate.snapshot();
                let name = &runtime.core.graph.routers[0].name;
                assert!(runtime.client(&format!("{name}-primary")).is_some());
                assert!(
                    runtime
                        .core
                        .balancers
                        .contains_key(&format!("{name}-backup"))
                );
                assert!(runtime.core.limiter.has_limit(&format!("{name}-rate")));
                // Requests use distinct buckets so this test exercises generation
                // consistency rather than exhaustion of the small fixture limits.
                let mut req = request(&gate);
                req.extensions_mut().insert(axum::extract::ConnectInfo(
                    format!("10.{reader}.0.{}:12345", iteration + 1)
                        .parse::<std::net::SocketAddr>()
                        .unwrap(),
                ));
                let response = crate::api::gateway::service(req).await.unwrap();
                assert_eq!(response.status(), 200);
                let limit = response.headers()["x-ratelimit-limit"].clone();
                let body = response.into_body().collect().await.unwrap().to_bytes();
                assert!(matches!(
                    (limit.to_str().unwrap(), body.as_ref()),
                    ("11", b"old backup") | ("22", b"new backup")
                ));
            }
        });
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(gate.snapshot().version, 32);
}

fn with_probe(config: gate::cfg::Config) -> gate::cfg::Config {
    let mut value = serde_json::to_value(config).unwrap();
    value["http"]["upstreams"]["secure"]["load_balancer"]["liveness_probe"] =
        serde_json::json!({"path": "/health", "interval": "10ms"});
    serde_json::from_value(value).unwrap()
}

fn health_count(upstream: &MtlsUpstream) -> usize {
    upstream
        .paths
        .lock()
        .unwrap()
        .iter()
        .filter(|path| path.as_str() == "/health")
        .count()
}

async fn wait_for_health_count(upstream: &MtlsUpstream, count: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while health_count(upstream) < count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("health probes stopped unexpectedly");
}

#[tokio::test]
async fn failed_preparation_retains_active_probes_and_success_retires_the_old_generation() {
    let old = MtlsUpstream::generation(false, "old").await;
    let new = MtlsUpstream::generation(false, "new").await;
    let files = TlsFiles::new();
    let raw = with_probe(files.config(&old.url));
    let gate = super::test_support::gate(raw.clone());
    let pinned = gate.snapshot();
    wait_for_health_count(&old, 2).await;
    let path = files.path("config.yaml");
    raw.to_file(&path);
    std::fs::remove_file(files.path("client-key.pem")).unwrap();
    assert!(reload_gateway_config_from_path(&gate, &path).is_err());
    assert!(Arc::ptr_eq(&pinned, &gate.snapshot()));
    assert_eq!(gate.snapshot().version, 0);
    wait_for_health_count(&old, health_count(&old) + 2).await;

    let mut candidate = with_probe(files.config(&new.url));
    candidate.mtls = None;
    gate.activate(super::prepare_config(candidate).unwrap())
        .unwrap();
    wait_for_health_count(&new, 3).await;
    let stopped = health_count(&old);
    wait_for_health_count(&new, 6).await;
    assert_eq!(
        health_count(&old),
        stopped,
        "retained requests must not retain old probe tasks"
    );
    assert_eq!(pinned.version, 0);
    assert_eq!(gate.snapshot().version, 1);
}
