use super::transport_tests::{execution, runtime};
use super::*;
#[cfg(feature = "memory")]
use crate::etc::gate::test_support;
use crate::etc::gate::test_support::{CA, CLIENT_CERT, SERVER_CERT, SERVER_KEY, TlsFiles};
use http::header::ORIGIN;
use std::{io::BufReader, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request as ServerRequest, Response as ServerResponse},
    },
};

#[derive(Debug)]
struct CapturedHandshake {
    headers: http::HeaderMap,
    server_name: Option<String>,
    alpn: Option<Vec<u8>>,
    client_cert: Vec<u8>,
}

struct TlsUpstream {
    url: String,
    captured: mpsc::UnboundedReceiver<CapturedHandshake>,
    task: JoinHandle<()>,
}

impl TlsUpstream {
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
        let chain = rustls_pemfile::certs(&mut BufReader::new(SERVER_CERT))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let key = rustls_pemfile::private_key(&mut BufReader::new(SERVER_KEY))
            .unwrap()
            .unwrap();
        let mut tls = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(chain, key)
            .unwrap();
        // Advertise h2 first: the WebSocket connector must still choose HTTP/1.1.
        tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "https://localhost:{}",
            listener.local_addr().unwrap().port()
        );
        let (sender, captured) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let acceptor = acceptor.clone();
                        let sender = sender.clone();
                        connections.spawn(async move {
                            let Ok(stream) = acceptor.accept(stream).await else { return; };
                            let session = stream.get_ref().1;
                            let server_name = session.server_name().map(str::to_owned);
                            let alpn = session.alpn_protocol().map(Vec::from);
                            let client_cert = session.peer_certificates().unwrap()[0].to_vec();
                            #[allow(clippy::result_large_err)]
                            let callback = move |req: &ServerRequest, mut response: ServerResponse| {
                                if req.headers().contains_key(SEC_WEBSOCKET_PROTOCOL) {
                                    response.headers_mut().insert(SEC_WEBSOCKET_PROTOCOL, "superchat".parse().unwrap());
                                }
                                sender.send(CapturedHandshake {headers: req.headers().clone(), server_name, alpn, client_cert}).unwrap();
                                Ok(response)
                            };
                            let Ok(mut socket) = accept_hdr_async(stream, callback).await else { return; };
                            while let Some(Ok(message)) = socket.next().await {
                                let close = message.is_close();
                                if socket.send(message).await.is_err() || close { break; }
                            }
                        });
                    }
                    Some(result) = connections.join_next(), if !connections.is_empty() => { result.unwrap(); }
                }
            }
        });
        Self {
            url,
            captured,
            task,
        }
    }
}

impl Drop for TlsUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn prepared_websocket_tls_uses_private_ca_identity_target_name_and_pinned_files() {
    let mut upstream = TlsUpstream::start().await;
    let files = TlsFiles::new();
    let url = format!("{}/socket", upstream.url.replacen("https://", "wss://", 1));
    let good = runtime(&url, &["http1", "http2"], "1s", Some(files.mtls()));
    let public = runtime(&url, &[], "1s", None);
    let mut wrong_identity = files.mtls();
    wrong_identity.client_cert_path = files.write("server.pem", SERVER_CERT);
    wrong_identity.client_key_path = files.write("server-key.pem", SERVER_KEY);
    let wrong = runtime(&url, &[], "1s", Some(wrong_identity));
    // Activation pins TLS material; a live request never rereads these files.
    std::fs::remove_file(files.path("client-key.pem")).unwrap();
    let mut incoming = http::HeaderMap::new();
    incoming.insert(HOST, "gateway.invalid:8443".parse().unwrap());
    incoming.insert(ORIGIN, "https://client.example".parse().unwrap());
    incoming.append(SEC_WEBSOCKET_PROTOCOL, "chat".parse().unwrap());
    incoming.append(SEC_WEBSOCKET_PROTOCOL, "superchat".parse().unwrap());
    for runtime in [&public, &wrong] {
        let request = build_upstream_request(&url, &incoming, true, None).unwrap();
        let error = connect_upstream(
            request,
            runtime.transport("socket").unwrap(),
            &execution(runtime),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::UpstreamConnectionFailed);
    }
    let request = build_upstream_request(&url, &incoming, true, None).unwrap();
    let (mut socket, response) = connect_upstream(
        request,
        good.transport("socket").unwrap(),
        &execution(&good),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert_eq!(response.headers()[SEC_WEBSOCKET_PROTOCOL], "superchat");
    socket
        .send(Message::Text("secure echo".into()))
        .await
        .unwrap();
    assert_eq!(
        socket.next().await.unwrap().unwrap(),
        Message::Text("secure echo".into())
    );
    let captured = upstream.captured.recv().await.unwrap();
    assert_eq!(captured.headers[HOST], "gateway.invalid:8443");
    assert_eq!(captured.headers.get_all(HOST).iter().count(), 1);
    assert_eq!(captured.headers[ORIGIN], "https://client.example");
    assert_eq!(captured.headers[SEC_WEBSOCKET_PROTOCOL], "chat, superchat");
    assert_eq!(captured.server_name.as_deref(), Some("localhost"));
    assert_eq!(captured.alpn.as_deref(), Some(b"http/1.1".as_slice()));
    assert_eq!(
        captured.client_cert,
        rustls_pemfile::certs(&mut BufReader::new(CLIENT_CERT))
            .next()
            .unwrap()
            .unwrap()
            .to_vec()
    );
    socket.close(None).await.unwrap();
}

#[cfg(feature = "memory")]
#[tokio::test]
async fn gateway_mtls_upgrade_negotiates_protocol_proxies_frames_and_releases_admission_on_disconnect()
 {
    let mut upstream = TlsUpstream::start().await;
    let files = TlsFiles::new();
    let mut config = files.config(&upstream.url);
    config.runtime.response_body_idle_timeout = "5s".into();
    config.http.middlewares.insert(
        "host".into(),
        serde_json::from_value(serde_json::json!({"kind":"preserve_host"})).unwrap(),
    );
    config.http.routers.get_mut("secure").unwrap().middlewares = vec!["host".into()];
    let gate = Arc::new(test_support::gate(config));
    let available = gate.resources.available().0;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/socket", listener.local_addr().unwrap());
    let app = axum::Router::new()
        .fallback(super::super::service)
        .layer(axum::Extension(gate.clone()));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let mut request = url.as_str().into_client_request().unwrap();
    request
        .headers_mut()
        .insert(HOST, "gateway.invalid".parse().unwrap());
    request
        .headers_mut()
        .insert(ORIGIN, "https://client.example".parse().unwrap());
    request
        .headers_mut()
        .insert(SEC_WEBSOCKET_PROTOCOL, "chat, superchat".parse().unwrap());
    let (mut socket, response) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(response.headers()[SEC_WEBSOCKET_PROTOCOL], "superchat");
    let captured = upstream.captured.recv().await.unwrap();
    assert_eq!(captured.headers[HOST], "gateway.invalid");
    assert_eq!(captured.headers.get_all(HOST).iter().count(), 1);
    assert_eq!(captured.headers[ORIGIN], "https://client.example");
    assert_eq!(gate.resources.available().0, available - 1);
    socket
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap();
    assert_eq!(
        socket.next().await.unwrap().unwrap(),
        Message::Binary(vec![1, 2, 3].into())
    );
    socket.close(None).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .is_close()
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        while gate.resources.available().0 != available {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    gate.resources.begin_shutdown();
    tokio::time::timeout(Duration::from_secs(1), gate.resources.wait())
        .await
        .unwrap();
    server.abort();
}
