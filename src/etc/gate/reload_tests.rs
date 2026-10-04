use super::test_support::{CA, CLIENT_KEY, SERVER_CERT, SERVER_KEY, TlsFiles};
use super::{
    CachedConfig, GatewayPreparationError, load_config_from_path, reload_gateway_config_from_path,
    transport::TransportPreparationError, update_cached_config,
};
use axum::body::Body;
use gate::{Gate, PolicySnapshot};
use http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use hyper::{server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use std::{
    convert::Infallible,
    io::BufReader,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};

struct MtlsUpstream {
    url: String,
    task: JoinHandle<()>,
}

impl MtlsUpstream {
    async fn start() -> Self {
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
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("https://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (socket, _) = accepted.unwrap();
                        let acceptor = acceptor.clone();
                        connections.spawn(async move {
                            let stream = acceptor.accept(socket).await.unwrap();
                            let service = service_fn(|_| async {
                                // Force every probe to complete a new authenticated TLS handshake.
                                Ok::<_, Infallible>(Response::builder()
                                    .header("connection", "close")
                                    .body(Body::from("mtls ready")).unwrap())
                            });
                            http1::Builder::new().serve_connection(TokioIo::new(stream), service)
                                .await.unwrap();
                        });
                    }
                    Some(result) = connections.join_next(), if !connections.is_empty() => {
                        result.unwrap();
                    }
                }
            }
        });
        Self { url, task }
    }
}

impl Drop for MtlsUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn assert_upstream_works(cache: &RwLock<Option<CachedConfig>>, url: &str) {
    let client = cache
        .read()
        .unwrap()
        .as_ref()
        .unwrap()
        .prepared
        .transports
        .get("secure")
        .unwrap();
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
    let mut gate =
        Gate::new(Arc::new(lim::State::new())).build(&prepared.config, PolicySnapshot::default());
    let cache = RwLock::new(None);
    update_cached_config(&cache, prepared);
    let graph = gate.http_graph.load_full();
    let balancers = gate.http_balancers.load_full();
    let limiter = gate.limiter.load_full();
    assert_upstream_works(&cache, &upstream.url).await;

    raw.http.routers.get_mut("secure").unwrap().priority = Some(123);
    raw.to_file(&path);
    std::fs::remove_file(files.path("client-key.pem")).unwrap();
    let error = reload_gateway_config_from_path(&mut gate, &path, &cache)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        GatewayPreparationError::Transport(TransportPreparationError::ReadFile {
            kind: "client key",
            ..
        })
    ));
    assert!(!cache.is_poisoned());
    assert_eq!(cache.read().unwrap().as_ref().unwrap().version, 0);
    assert!(Arc::ptr_eq(&graph, &gate.http_graph.load_full()));
    assert!(Arc::ptr_eq(&balancers, &gate.http_balancers.load_full()));
    assert!(Arc::ptr_eq(&limiter, &gate.limiter.load_full()));
    assert_eq!(gate.http_graph.load().routers[0].priority, 0);
    assert_upstream_works(&cache, &upstream.url).await;

    files.write("client-key.pem", CLIENT_KEY);
    assert_eq!(
        reload_gateway_config_from_path(&mut gate, &path, &cache)
            .await
            .unwrap(),
        1
    );
    assert!(!Arc::ptr_eq(&graph, &gate.http_graph.load_full()));
    assert_eq!(gate.http_graph.load().routers[0].priority, 123);
    assert_upstream_works(&cache, &upstream.url).await;
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
