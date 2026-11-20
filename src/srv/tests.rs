use super::*;
use crate::api::plane::PlaneSet;
use axum::{Router, body::Body, response::Response};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject};
use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// A listener bound on an ephemeral port, with the task that serves it.
struct Server {
    address: SocketAddr,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Server {
    async fn start(
        settings: settings::Listener,
        router: Router,
        tls: Option<Arc<rustls::ServerConfig>>,
        http: HttpSettings,
        timeout: Duration,
    ) -> Self {
        let bound = bind(vec![Listener { settings, router }], tls.as_ref())
            .await
            .unwrap();
        let address = bound[0].address;
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(run(
            bound,
            http,
            async {
                let _ = stopped.await;
            },
            timeout,
        ));
        Self {
            address,
            stop: Some(stop),
            task: Some(task),
        }
    }

    async fn shutdown(&mut self) {
        let _ = self.stop.take().expect("not yet shut down").send(());
        let task = self.task.take().expect("not yet shut down");
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("server stops within the test deadline")
            .unwrap();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn listener(tls: bool, max_connections: usize) -> settings::Listener {
    settings::Listener {
        name: "test".to_string(),
        address: "127.0.0.1:0".parse().unwrap(),
        planes: PlaneSet::ALL,
        tls,
        max_connections,
        trusted_proxies: Default::default(),
    }
}

fn plain_listener() -> settings::Listener {
    listener(false, 32)
}

fn tls_listener() -> settings::Listener {
    listener(true, 32)
}

fn echo_router() -> Router {
    Router::new().fallback(|| async { "stargate" })
}

fn short_header_timeout() -> HttpSettings {
    HttpSettings {
        header_read_timeout: Duration::from_millis(300),
        ..HttpSettings::default()
    }
}

fn server_tls_config() -> Arc<rustls::ServerConfig> {
    tls::install_crypto_provider();
    let certs = CertificateDer::pem_slice_iter(crate::gate::test_support::SERVER_CERT)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(crate::gate::test_support::SERVER_KEY).unwrap();
    tls::server_config(certs, key).unwrap()
}

fn client_tls_config() -> Arc<rustls::ClientConfig> {
    tls::install_crypto_provider();
    let mut roots = rustls::RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(crate::gate::test_support::CA) {
        roots.add(certificate.unwrap()).unwrap();
    }
    let mut config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Arc::new(config)
}

/// Reads until the end of the response headers, or returns what arrived before
/// the peer closed.
async fn read_headers(stream: &mut TcpStream, within: Duration) -> String {
    let mut response = Vec::new();
    let read = async {
        loop {
            let mut buffer = [0u8; 1024];
            let read = stream.read(&mut buffer).await.unwrap();
            if read == 0 {
                return;
            }
            response.extend_from_slice(&buffer[..read]);
            if response.windows(4).any(|window| window == b"\r\n\r\n") {
                return;
            }
        }
    };
    tokio::time::timeout(within, read)
        .await
        .expect("a response within the deadline");
    String::from_utf8_lossy(&response).into_owned()
}

async fn read_until_eof<R: AsyncRead + Unpin>(stream: &mut R, within: Duration) {
    let read = async {
        loop {
            let mut buffer = [0u8; 256];
            // A transport error is a closed connection too: dropping the socket
            // without a TLS close_notify surfaces here.
            match stream.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    };
    tokio::time::timeout(within, read)
        .await
        .expect("the peer closes the connection");
}

#[tokio::test]
async fn requests_carry_the_peer_and_the_listener_they_arrived_on() {
    let router = Router::new().fallback(
        |axum::Extension(connection): axum::Extension<ConnectionInfo>| async move {
            format!(
                "{} {} {} {}",
                connection.peer.ip(),
                connection.listener.name,
                connection.listener.planes,
                connection.listener.tls
            )
        },
    );
    let server = Server::start(
        plain_listener(),
        router,
        None,
        HttpSettings::default(),
        Duration::from_secs(1),
    )
    .await;
    let mut client = TcpStream::connect(server.address).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let response = read_headers(&mut client, Duration::from_secs(5)).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(
        response.contains("127.0.0.1 test management,identity,admin,gateway false"),
        "{response}"
    );
}

#[tokio::test]
async fn incomplete_request_headers_are_closed_at_the_read_timeout() {
    let http = HttpSettings {
        header_read_timeout: Duration::from_millis(300),
        ..HttpSettings::default()
    };
    let server = Server::start(
        plain_listener(),
        echo_router(),
        None,
        http,
        Duration::from_secs(1),
    )
    .await;

    let mut complete = TcpStream::connect(server.address).await.unwrap();
    complete
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let response = read_headers(&mut complete, Duration::from_secs(5)).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");

    let mut slow = TcpStream::connect(server.address).await.unwrap();
    slow.write_all(b"GET / HTTP/1.1\r\nHost: loc")
        .await
        .unwrap();
    read_until_eof(&mut slow, Duration::from_secs(5)).await;
}

#[tokio::test]
async fn a_mute_connection_is_closed_at_the_first_request_timeout() {
    let server = Server::start(
        plain_listener(),
        echo_router(),
        None,
        short_header_timeout(),
        Duration::from_secs(1),
    )
    .await;
    let mut mute = TcpStream::connect(server.address).await.unwrap();
    read_until_eof(&mut mute, Duration::from_secs(5)).await;
}

#[tokio::test]
async fn a_connection_stopped_after_the_http2_preface_is_closed() {
    let server = Server::start(
        plain_listener(),
        echo_router(),
        None,
        short_header_timeout(),
        Duration::from_secs(1),
    )
    .await;
    let mut stalled = TcpStream::connect(server.address).await.unwrap();
    stalled.write_all(b"PRI * HTTP/2.0\r\n").await.unwrap();
    read_until_eof(&mut stalled, Duration::from_secs(5)).await;
}

#[tokio::test]
async fn a_tls_connection_that_stays_silent_is_closed_at_the_first_request_timeout() {
    let server = Server::start(
        tls_listener(),
        echo_router(),
        Some(server_tls_config()),
        short_header_timeout(),
        Duration::from_secs(1),
    )
    .await;
    let stream = TcpStream::connect(server.address).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(client_tls_config());
    let mut tls = connector
        .connect(ServerName::try_from("localhost").unwrap(), stream)
        .await
        .unwrap();
    read_until_eof(&mut tls, Duration::from_secs(5)).await;
}

#[tokio::test]
async fn a_mute_connection_frees_its_slot_at_the_first_request_timeout() {
    let server = Server::start(
        listener(false, 1),
        echo_router(),
        None,
        short_header_timeout(),
        Duration::from_secs(1),
    )
    .await;
    let mut mute = TcpStream::connect(server.address).await.unwrap();
    read_until_eof(&mut mute, Duration::from_secs(5)).await;

    let mut client = TcpStream::connect(server.address).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let response = read_headers(&mut client, Duration::from_secs(5)).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

#[tokio::test]
async fn incomplete_tls_handshakes_are_closed_at_the_handshake_timeout() {
    let http = HttpSettings {
        tls_handshake_timeout: Duration::from_millis(300),
        ..HttpSettings::default()
    };
    let server = Server::start(
        tls_listener(),
        echo_router(),
        Some(server_tls_config()),
        http,
        Duration::from_secs(1),
    )
    .await;
    let mut client = TcpStream::connect(server.address).await.unwrap();
    client.write_all(b"\x16\x03\x03\x00\x14\x01").await.unwrap();
    read_until_eof(&mut client, Duration::from_secs(5)).await;
}

#[tokio::test]
async fn tls_negotiates_and_serves_http2() {
    let server = Server::start(
        tls_listener(),
        Router::new().route("/", axum::routing::get(|| async { "h2" })),
        Some(server_tls_config()),
        HttpSettings::default(),
        Duration::from_secs(1),
    )
    .await;

    let stream = TcpStream::connect(server.address).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(client_tls_config());
    let tls = connector
        .connect(ServerName::try_from("localhost").unwrap(), stream)
        .await
        .unwrap();
    assert_eq!(
        tls.get_ref().1.alpn_protocol(),
        Some(b"h2".as_slice()),
        "the server offers HTTP/2 through ALPN"
    );

    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
            .await
            .unwrap();
    tokio::spawn(connection);
    let request = http::Request::builder()
        .uri("https://localhost/")
        .body(http_body_util::Empty::<hyper::body::Bytes>::new())
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(response.version(), http::Version::HTTP_2);
}

#[tokio::test]
async fn a_listener_serves_at_most_its_connection_cap() {
    let server = Server::start(
        listener(false, 1),
        echo_router(),
        None,
        HttpSettings::default(),
        Duration::from_secs(1),
    )
    .await;

    let mut first = TcpStream::connect(server.address).await.unwrap();
    first
        .write_all(b"GET /first HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    assert!(
        read_headers(&mut first, Duration::from_secs(5))
            .await
            .starts_with("HTTP/1.1 200")
    );

    // The kernel completes the handshake, but the listener has no slot until
    // the first connection closes.
    let mut second = TcpStream::connect(server.address).await.unwrap();
    second
        .write_all(b"GET /second HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut buffer = [0u8; 1024];
    assert!(
        tokio::time::timeout(Duration::from_millis(300), second.read(&mut buffer))
            .await
            .is_err(),
        "the second connection must wait for the only slot"
    );

    drop(first);
    let read = tokio::time::timeout(Duration::from_secs(5), second.read(&mut buffer))
        .await
        .expect("the second connection is served once a slot frees")
        .unwrap();
    assert!(
        String::from_utf8_lossy(&buffer[..read]).starts_with("HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&buffer[..read])
    );
}

/// Accepts nothing and always fails, like a listener whose descriptor table is
/// full.
struct Failing {
    attempts: Arc<AtomicUsize>,
}

impl Accept for Failing {
    async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Err(io::Error::other("accept failed"))
    }
}

#[tokio::test(start_paused = true)]
async fn accept_errors_back_off_instead_of_spinning() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let bound = Bound {
        info: Arc::new(ListenerInfo {
            name: "failing".to_string(),
            planes: PlaneSet::ALL,
            tls: false,
            trusted_proxies: Default::default(),
        }),
        address: "127.0.0.1:1".parse().unwrap(),
        tls: None,
        max_connections: 8,
        socket: Failing {
            attempts: Arc::clone(&attempts),
        },
        router: Router::new(),
    };
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(run(
        vec![bound],
        HttpSettings::default(),
        async {
            let _ = stopped.await;
        },
        Duration::from_millis(100),
    ));

    tokio::task::yield_now().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    tokio::time::advance(ACCEPT_ERROR_BACKOFF / 2).await;
    tokio::task::yield_now().await;
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "the loop must wait before retrying"
    );
    tokio::time::advance(ACCEPT_ERROR_BACKOFF).await;
    tokio::task::yield_now().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 2);

    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn shutdown_deadline_drops_stalled_responses_and_keeps_the_connection_peer() {
    let dropped = Arc::new(AtomicBool::new(false));
    let flag = dropped.clone();
    let app = axum::Router::new().fallback(
        move |axum::Extension(connection): axum::Extension<ConnectionInfo>| {
            let body_guard = Dropped(flag.clone());
            async move {
                assert!(connection.peer.ip().is_loopback());
                let frames = futures_util::stream::unfold(body_guard, |guard| async move {
                    let _guard = guard;
                    std::future::pending::<
                        Option<(Result<hyper::body::Bytes, Infallible>, Dropped)>,
                    >()
                    .await
                });
                Response::new(Body::from_stream(frames))
            }
        },
    );
    let mut server = Server::start(
        plain_listener(),
        app,
        None,
        HttpSettings::default(),
        Duration::from_secs(1),
    )
    .await;
    let mut client = TcpStream::connect(server.address).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    read_headers(&mut client, Duration::from_secs(5)).await;

    server.shutdown().await;
    assert!(dropped.load(Ordering::SeqCst));
    let mut buffer = [0u8; 64];
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), client.read(&mut buffer))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn shutdown_cancels_an_incomplete_inbound_tls_handshake() {
    let mut server = Server::start(
        tls_listener(),
        Router::new(),
        Some(server_tls_config()),
        HttpSettings::default(),
        Duration::from_secs(1),
    )
    .await;
    let mut client = TcpStream::connect(server.address).await.unwrap();
    client.write_all(b"\x16\x03\x03\x00\x14\x01").await.unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    server.shutdown().await;
}
