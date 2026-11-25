use crate::{
    api::{
        listener_router,
        plane::{Plane, PlaneSet},
        server_router,
    },
    conf::hosts::IamHosts,
    ctx::RequestContext,
    gate::test_support,
    srv::{self, HttpSettings, Listener, health::Lifecycle, settings},
    web::request::RequestExt,
};
use axum::{Extension, Router, body::Body, middleware::from_fn, response::Response, routing::get};
use http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
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
async fn a_listener_serves_only_the_planes_it_declares() {
    let upstream = Upstream::start().await;
    let gate = Arc::new(test_support::gate(config(&upstream.url)));
    let identity = spawn_listener(
        "identity",
        PlaneSet::parse("identity").unwrap(),
        gate.clone(),
    )
    .await;
    let api = spawn_listener(
        "api",
        PlaneSet::parse("admin,gateway").unwrap(),
        gate.clone(),
    )
    .await;
    let probes = spawn_listener("probes", PlaneSet::parse("management").unwrap(), gate).await;

    // Identity: its own routes only; no admin and no gateway fallback.
    assert_eq!(
        status(identity.addresses[0], "/docs").await,
        StatusCode::OK.as_u16()
    );
    assert_eq!(
        status(identity.addresses[0], "/admin/health").await,
        StatusCode::NOT_FOUND.as_u16()
    );
    assert_eq!(
        status(identity.addresses[0], "/assets/").await,
        StatusCode::NOT_FOUND.as_u16()
    );
    assert_eq!(
        status(identity.addresses[0], "/livez").await,
        StatusCode::NOT_FOUND.as_u16()
    );

    // Admin and gateway: the admin API answers and everything else is proxied.
    assert_eq!(
        status(api.addresses[0], "/admin/health").await,
        StatusCode::FORBIDDEN.as_u16()
    );
    assert_eq!(
        status(api.addresses[0], "/assets/").await,
        StatusCode::OK.as_u16()
    );

    // Management: probes, and nothing else.
    assert_eq!(
        status(probes.addresses[0], "/livez").await,
        StatusCode::OK.as_u16()
    );
    assert_eq!(
        status(probes.addresses[0], "/docs").await,
        StatusCode::NOT_FOUND.as_u16()
    );

    identity.stop().await;
    api.stop().await;
    probes.stop().await;
}

/// The bodies `/livez` and `/readyz` answer with on a listener that serves the
/// probes. The lifecycle starts unready, so readiness fails without SQL I/O.
const PROBE_BODIES: [&str; 2] = [r#"{"status":"alive"}"#, r#"{"status":"not_ready"}"#];

async fn probe_bodies(address: SocketAddr) -> [String; 2] {
    [
        body(address, "/livez").await,
        body(address, "/readyz").await,
    ]
}

/// A listener dedicated to the management plane answers the probes; the public
/// listener leaves the paths to its gateway, so no probe answers there.
#[tokio::test]
async fn a_dedicated_management_listener_takes_the_probes_off_the_other_listeners() {
    let upstream = Upstream::start().await;
    let gate = Arc::new(test_support::gate(config(&upstream.url)));
    let listeners = settings::assign_management_plane(vec![
        listener_settings(
            "public",
            PlaneSet::parse("identity,gateway").unwrap(),
            false,
            Default::default(),
        ),
        listener_settings(
            "management",
            PlaneSet::of(Plane::Management),
            false,
            Default::default(),
        ),
    ]);
    let routers = listeners
        .into_iter()
        .map(|settings| {
            let router =
                listener_router(settings.planes, Lifecycle::default(), IamHosts::default())
                    .layer(Extension(gate.clone()));
            (settings, router)
        })
        .collect();
    let running = spawn_listeners(routers, None).await;

    assert_eq!(probe_bodies(running.addresses[1]).await, PROBE_BODIES);
    assert_eq!(
        probe_bodies(running.addresses[0]).await,
        ["GET /livez 0", "GET /readyz 0"]
    );

    running.stop().await;
}

/// Without a listener dedicated to the management plane, every listener keeps
/// the probes: splitting the planes never leaves the process without liveness
/// and readiness.
#[tokio::test]
async fn the_probes_stay_on_every_listener_without_a_dedicated_one() {
    let upstream = Upstream::start().await;
    let gate = Arc::new(test_support::gate(config(&upstream.url)));
    let listeners = settings::assign_management_plane(vec![
        listener_settings(
            "public",
            PlaneSet::parse("identity,gateway").unwrap(),
            false,
            Default::default(),
        ),
        listener_settings(
            "admin",
            PlaneSet::parse("admin").unwrap(),
            false,
            Default::default(),
        ),
    ]);
    let routers = listeners
        .into_iter()
        .map(|settings| {
            let router =
                listener_router(settings.planes, Lifecycle::default(), IamHosts::default())
                    .layer(Extension(gate.clone()));
            (settings, router)
        })
        .collect();
    let running = spawn_listeners(routers, None).await;

    for address in &running.addresses {
        assert_eq!(probe_bodies(*address).await, PROBE_BODIES);
    }

    running.stop().await;
}

/// The implicit listener serves every plane already, probes included, and the
/// management rule leaves it unchanged.
#[tokio::test]
async fn the_implicit_listener_keeps_serving_the_probes() {
    let upstream = Upstream::start().await;
    let gate = Arc::new(test_support::gate(config(&upstream.url)));
    let implicit = PlaneSet::ALL;
    let listeners = spawn_listeners(
        vec![(
            listener_settings("default", implicit, false, Default::default()),
            listener_router(implicit, Lifecycle::default(), IamHosts::default())
                .layer(Extension(gate)),
        )],
        None,
    )
    .await;

    assert_eq!(probe_bodies(listeners.addresses[0]).await, PROBE_BODIES);

    listeners.stop().await;
}

#[tokio::test]
async fn a_gateway_only_listener_forwards_the_client_path_unchanged() {
    let upstream = Upstream::start().await;
    let gate = Arc::new(test_support::gate(config(&upstream.url)));
    let gateway = spawn_listener("gateway", PlaneSet::parse("gateway").unwrap(), gate).await;

    // No normalisation and no IAM routes: the upstream receives the path the
    // client sent, and the server's own routes are not served here.
    for path in ["/assets/", "/docs/"] {
        assert_eq!(
            body(gateway.addresses[0], path).await,
            format!("GET {path} 0"),
            "{path}"
        );
    }

    gateway.stop().await;
}

struct RunningListeners {
    addresses: Vec<SocketAddr>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl RunningListeners {
    async fn stop(mut self) {
        let _ = self.stop.take().expect("not yet shut down").send(());
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .expect("the listeners stop")
            .unwrap();
    }
}

fn listener_settings(
    name: &str,
    planes: PlaneSet,
    tls: bool,
    trusted_proxies: crate::srv::proxy::ProxyTrust,
) -> settings::Listener {
    settings::Listener {
        name: name.to_string(),
        address: "127.0.0.1:0".parse().unwrap(),
        planes,
        tls,
        max_connections: 16,
        trusted_proxies,
    }
}

async fn spawn_listeners(
    listeners: Vec<(settings::Listener, Router)>,
    tls: Option<Arc<rustls::ServerConfig>>,
) -> RunningListeners {
    let bound = srv::bind(
        listeners
            .into_iter()
            .map(|(settings, router)| Listener { settings, router })
            .collect(),
        tls.as_ref(),
    )
    .await
    .unwrap();
    let addresses = bound.iter().map(|listener| listener.address).collect();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(srv::run(
        bound,
        HttpSettings::default(),
        async move {
            let _ = stopped.await;
        },
        Duration::from_secs(1),
    ));
    RunningListeners {
        addresses,
        stop: Some(stop),
        task,
    }
}

async fn spawn_listener(
    name: &str,
    planes: PlaneSet,
    gate: Arc<crate::gate::Gate>,
) -> RunningListeners {
    let router =
        listener_router(planes, Lifecycle::default(), IamHosts::default()).layer(Extension(gate));
    spawn_listeners(
        vec![(
            listener_settings(name, planes, false, Default::default()),
            router,
        )],
        None,
    )
    .await
}

async fn body(address: SocketAddr, path: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
        .await
        .expect("a response within the deadline")
        .unwrap();
    let response = String::from_utf8_lossy(&response);
    let (_, body) = response.split_once("\r\n\r\n").expect("headers and a body");
    body.to_string()
}

async fn status(address: SocketAddr, path: &str) -> u16 {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    while !response.windows(4).any(|window| window == b"\r\n\r\n") {
        let mut buffer = [0u8; 1024];
        let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
            .await
            .expect("a response within the deadline")
            .unwrap();
        assert!(read > 0, "the listener closed before responding");
        response.extend_from_slice(&buffer[..read]);
    }
    let response = String::from_utf8_lossy(&response);
    response.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn plane_sets_parse_and_print_their_planes() {
    let planes = PlaneSet::parse("identity, gateway").unwrap();
    assert!(planes.contains(crate::api::plane::Plane::Identity));
    assert!(planes.contains(crate::api::plane::Plane::Gateway));
    assert!(!planes.contains(crate::api::plane::Plane::Admin));
    assert_eq!(planes.to_string(), "identity,gateway");
    assert_eq!(
        PlaneSet::parse("management").unwrap().to_string(),
        "management"
    );
    assert!(PlaneSet::parse("").is_err());
    assert!(PlaneSet::parse("identity,").is_err());
    assert!(PlaneSet::parse("control").is_err());
    assert!(PlaneSet::parse("identity,identity").is_err());
    assert_eq!(
        PlaneSet::parse("ADMIN").unwrap(),
        PlaneSet::of(crate::api::plane::Plane::Admin)
    );
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
async fn identity_cors_admits_the_console_origin_on_its_paths_only() {
    let upstream = Upstream::start().await;
    let app = app_for(&upstream, IamHosts::default());
    // The Console origin defaults to the public base URL.
    let console = crate::web::origin::origin_of_url(&crate::conf::apps::console_public_url())
        .unwrap()
        .to_url();

    for (method, path) in [
        ("POST", "/oauth/token"),
        ("POST", "/oauth/revoke"),
        ("GET", "/i18n/en"),
        ("GET", "/account/profile"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::ORIGIN, console.as_str())
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            console,
            "{method} {path}"
        );
    }

    let preflight = app
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/account/profile")
                .header(header::ORIGIN, console.as_str())
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PUT")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        preflight.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        console
    );

    let response = app
        .oneshot(
            Request::get("/account/consents")
                .header(header::ORIGIN, console.as_str())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        !response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
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

fn ingress_config(burst: u32) -> gate::cfg::Config {
    serde_json::from_value(serde_json::json!({
        "schema": gate::cfg::SCHEMA,
        "ingress": {"limit": "ingress", "timeout": "250ms"},
        "limits": {
            "ingress": {"strategy": "gcra", "params": {"max_burst": burst, "replenish_1_per": "1h"}},
            "default": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "1h"}},
        },
    }))
    .unwrap()
}

/// Reports the same client address the traces log, the ingress limit charges,
/// and audit records, plus whether the forwarding header survived ingress.
async fn client_ip_probe(req: axum::extract::Request) -> String {
    let audit = crate::ctx::audit_request_from(req.extensions());
    let context = req
        .extensions()
        .get::<RequestContext>()
        .expect("the trace middleware runs on every listener");
    format!(
        "log={} limit={} audit={} forwarded={}",
        context
            .client_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_default(),
        req.get_client_ip(),
        audit.raw_request().ip_address.clone().unwrap_or_default(),
        req.headers().contains_key("x-forwarded-for"),
    )
}

fn probe_router(burst: u32) -> Router {
    Router::new()
        .route("/client-ip", get(client_ip_probe))
        .layer(from_fn(
            crate::api::middleware::rate_limit::ingress_limit_middleware,
        ))
        .layer(from_fn(crate::obs::logging::trace_middleware))
        .layer(Extension(Arc::new(test_support::gate(ingress_config(
            burst,
        )))))
}

async fn response_with_forwarded(
    address: SocketAddr,
    path: &str,
    forwarded: &str,
) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!(
                "GET {path} HTTP/1.1\r\nHost: localhost\r\nX-Forwarded-For: {forwarded}\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let response = read_response(&mut stream).await;
    (
        response_status(&response),
        response_body(&response).to_string(),
    )
}

async fn read_response(stream: &mut tokio::net::TcpStream) -> String {
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
        .await
        .expect("a response within the deadline")
        .unwrap();
    String::from_utf8_lossy(&response).into_owned()
}

fn response_status(response: &str) -> u16 {
    response
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse().ok())
        .unwrap_or(0)
}

fn response_body(response: &str) -> &str {
    response.split_once("\r\n\r\n").map_or("", |(_, body)| body)
}

fn header_value<'a>(response: &'a str, name: &str) -> &'a str {
    response
        .split_once("\r\n\r\n")
        .map_or(response, |(head, _)| head)
        .lines()
        .find_map(|line| {
            let (header, value) = line.split_once(':')?;
            header.eq_ignore_ascii_case(name).then(|| value.trim())
        })
        .unwrap_or("")
}

/// Two listeners with the same handler: the trusted one accepts the proxy's
/// address, the internal one keeps the peer and drops the forged header from
/// logs, limits, and audit.
#[tokio::test]
async fn an_untrusted_listener_keeps_the_peer_in_logs_limits_and_audit() {
    let trusted = spawn_listeners(
        vec![
            (
                listener_settings(
                    "public",
                    PlaneSet::ALL,
                    false,
                    crate::srv::proxy::ProxyTrust::parse("127.0.0.1/32").unwrap(),
                ),
                probe_router(2),
            ),
            (
                listener_settings("internal", PlaneSet::ALL, false, Default::default()),
                probe_router(1),
            ),
        ],
        None,
    )
    .await;

    // Behind the trusted proxy each forwarded address is its own client.
    for address in ["203.0.113.1", "203.0.113.2"] {
        let (status, body) =
            response_with_forwarded(trusted.addresses[0], "/client-ip", address).await;
        assert_eq!(status, 200);
        assert_eq!(
            body,
            format!("log={address} limit={address} audit={address} forwarded=true")
        );
    }

    // A direct client cannot choose its address: every reader sees the peer,
    // the header is gone before routing, and a second claim shares its bucket.
    let (status, body) =
        response_with_forwarded(trusted.addresses[1], "/client-ip", "198.51.100.1").await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        "log=127.0.0.1 limit=127.0.0.1 audit=127.0.0.1 forwarded=false"
    );
    let (status, _) =
        response_with_forwarded(trusted.addresses[1], "/client-ip", "198.51.100.2").await;
    assert_eq!(status, 429);

    trusted.stop().await;
}

fn server_tls_config() -> Arc<rustls::ServerConfig> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
    crate::srv::tls::install_crypto_provider();
    let certificates = CertificateDer::pem_slice_iter(test_support::SERVER_CERT)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(test_support::SERVER_KEY).unwrap();
    crate::srv::tls::server_config(certificates, key).unwrap()
}

fn client_tls_config() -> Arc<rustls::ClientConfig> {
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    crate::srv::tls::install_crypto_provider();
    let mut roots = rustls::RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(test_support::CA) {
        roots.add(certificate.unwrap()).unwrap();
    }
    let mut config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Arc::new(config)
}

async fn https_response(address: SocketAddr, path: &str) -> String {
    let stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(client_tls_config());
    let mut tls = connector
        .connect(
            rustls::pki_types::ServerName::try_from("localhost").unwrap(),
            stream,
        )
        .await
        .unwrap();
    tls.write_all(
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .await
    .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut response))
        .await
        .expect("a response within the deadline")
        .unwrap();
    String::from_utf8_lossy(&response).into_owned()
}

async fn plain_response(address: SocketAddr, path: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    read_response(&mut stream).await
}

/// Behind a cleartext public URL only the listener that terminates TLS marks
/// the session cookie `Secure` and sends HSTS.
#[tokio::test]
async fn only_a_tls_listener_marks_cookies_secure_and_sends_hsts() {
    assert!(
        !crate::srv::tls::over_https(false, &crate::conf::public_base_url()),
        "this test needs a cleartext public URL"
    );
    let gate = Arc::new(test_support::gate(ingress_config(100)));
    let router = || {
        listener_router(PlaneSet::ALL, Lifecycle::default(), IamHosts::default())
            .layer(Extension(gate.clone()))
            .layer(from_fn(crate::web::headers::security_headers_middleware))
            .layer(from_fn(crate::obs::logging::trace_middleware))
    };
    let listeners = spawn_listeners(
        vec![
            (
                listener_settings("plain", PlaneSet::ALL, false, Default::default()),
                router(),
            ),
            (
                listener_settings("tls", PlaneSet::ALL, true, Default::default()),
                router(),
            ),
        ],
        Some(server_tls_config()),
    )
    .await;

    let plain = plain_response(listeners.addresses[0], "/oauth/logout").await;
    assert_eq!(response_status(&plain), 303);
    assert_eq!(header_value(&plain, "strict-transport-security"), "");
    assert!(!header_value(&plain, "set-cookie").contains("Secure"));

    let tls = https_response(listeners.addresses[1], "/oauth/logout").await;
    assert_eq!(response_status(&tls), 303);
    assert!(!header_value(&tls, "strict-transport-security").is_empty());
    assert!(header_value(&tls, "set-cookie").contains("Secure"));

    listeners.stop().await;
}

#[tokio::test]
async fn listeners_without_admin_reserve_control_plane_paths_on_iam_hosts() {
    let upstream = Upstream::start().await;
    let gate = Arc::new(test_support::gate(config(&upstream.url)));
    let hosts =
        IamHosts::parse(Some("identity.example.com"), "https://identity.example.com").unwrap();
    for planes in ["identity,gateway", "gateway"] {
        let router = listener_router(
            PlaneSet::parse(planes).unwrap(),
            Lifecycle::default(),
            hosts.clone(),
        )
        .layer(Extension(gate.clone()));
        for path in [
            "/admin",
            "/admin/",
            "/admin/me",
            "/admin/unknown",
            "/admin/me/",
            "/stargate",
            "/stargate/",
            "/stargate/users",
            "/stargate/assets/app.js",
        ] {
            for method in [Method::GET, Method::POST, Method::OPTIONS] {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri(path)
                            .header("host", "identity.example.com")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::NOT_FOUND, "{planes} {path}");
            }
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("host", "api.example.com")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "non-IAM host {path}");
            assert_eq!(text(response).await, format!("GET {path} 0"));
        }
    }
}

#[tokio::test]
async fn documentation_contains_only_the_listeners_planes() {
    let gateway = Arc::new(test_support::gate(gate::cfg::Config::default()));
    for planes in ["identity", "admin", "identity,admin", "admin,gateway"] {
        let router = listener_router(
            PlaneSet::parse(planes).unwrap(),
            Lifecycle::default(),
            IamHosts::default(),
        )
        .layer(Extension(gateway.clone()));
        let response = router.oneshot(request(Method::GET, "/docs")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let doc: serde_json::Value = serde_json::from_str(&text(response).await).unwrap();
        let paths = doc["paths"].as_object().unwrap();
        assert_eq!(paths.contains_key("/admin/me"), planes.contains("admin"));
        assert_eq!(
            paths.contains_key("/account/profile"),
            planes.contains("identity")
        );
        assert!(!paths.contains_key("/livez"));
    }
}

#[tokio::test]
async fn the_configured_console_path_is_reserved_on_public_listeners() {
    const CHILD: &str = "STARGATE_CUSTOM_CONSOLE_BOUNDARY_TEST";
    if std::env::var_os(CHILD).is_none() {
        let output = crate::conf::child_test::run(
            "api::plane::tests::the_configured_console_path_is_reserved_on_public_listeners",
            |command| {
                command
                    .env(CHILD, "1")
                    .env("CONSOLE_APP_BASE_PATH", "/control");
            },
        );
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let upstream = Upstream::start().await;
    let router = listener_router(
        PlaneSet::parse("identity,gateway").unwrap(),
        Lifecycle::default(),
        IamHosts::default(),
    )
    .layer(Extension(Arc::new(test_support::gate(config(
        &upstream.url,
    )))));
    for path in [
        "/control",
        "/control/",
        "/control/users",
        "/control/assets/app.js",
    ] {
        assert_eq!(
            router
                .clone()
                .oneshot(request(Method::GET, path))
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    let response = router
        .oneshot(request(Method::GET, "/stargate"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(text(response).await, "GET /stargate 0");
}
