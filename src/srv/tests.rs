use super::*;
use axum::{body::Body, response::Response};
use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn plain_shutdown_deadline_drops_stalled_response_and_preserves_connect_info() {
    let dropped = Arc::new(AtomicBool::new(false));
    let flag = dropped.clone();
    let app = axum::Router::new().fallback(move |ConnectInfo(peer): ConnectInfo<SocketAddr>| {
        let body_guard = Dropped(flag.clone());
        async move {
            assert!(peer.ip().is_loopback());
            let frames = futures_util::stream::unfold(body_guard, |guard| async move {
                let _guard = guard;
                std::future::pending::<Option<(Result<hyper::body::Bytes, Infallible>, Dropped)>>()
                    .await
            });
            Response::new(Body::from_stream(frames))
        }
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (signal, shutdown) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve(
        app,
        listener,
        async {
            let _ = shutdown.await;
        },
        1,
        None,
    ));
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut headers = vec![0; 1024];
    let len = client.read(&mut headers).await.unwrap();
    assert!(String::from_utf8_lossy(&headers[..len]).starts_with("HTTP/1.1 200"));
    signal.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(client.read(&mut headers).await.unwrap(), 0);
}

#[tokio::test]
async fn shutdown_cancels_incomplete_inbound_tls_handshake() {
    let certs = rustls_pemfile::certs(&mut &crate::etc::gate::test_support::SERVER_CERT[..])
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = rustls_pemfile::private_key(&mut &crate::etc::gate::test_support::SERVER_KEY[..])
        .unwrap()
        .unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (signal, shutdown) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve(
        axum::Router::new(),
        listener,
        async {
            let _ = shutdown.await;
        },
        1,
        Some(TlsAcceptor::from(Arc::new(config))),
    ));
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client.write_all(b"\x16\x03\x03\x00\x14\x01").await.unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    signal.send(()).unwrap();
    tokio::time::timeout(Duration::from_millis(500), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
