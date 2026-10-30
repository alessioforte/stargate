use super::*;
use crate::gate::test_support::{CA, CLIENT_CERT, CLIENT_KEY, SERVER_CERT, SERVER_KEY, TlsFiles};

#[test]
fn valid_mtls_and_default_transports_prepare() {
    let files = TlsFiles::new();
    let tls = build_mtls(&files.mtls()).unwrap();
    assert!(tls.alpn_protocols.is_empty());
    let config = RuntimeConfig::from_raw(files.config("https://localhost:8443")).unwrap();
    assert!(prepare(&config).unwrap().get("secure").is_some());
    assert!(prepare(&RuntimeConfig::from_raw(gate::cfg::Config::default()).unwrap()).is_ok());
}

#[test]
fn missing_empty_malformed_and_invalid_der_files_return_typed_errors() {
    let files = TlsFiles::new();
    let invalid_files = [
        files.path("missing.pem"),
        files.write("empty.pem", b""),
        files.write(
            "malformed.pem",
            b"-----BEGIN CERTIFICATE-----\n!invalid!\n-----END CERTIFICATE-----\n",
        ),
        files.write(
            "invalid-der.pem",
            b"-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n",
        ),
    ];
    for kind in ["CA", "client", "client key"] {
        for path in &invalid_files {
            let mut mtls = files.mtls();
            match kind {
                "CA" => mtls.ca_cert_path = path.clone(),
                "client" => mtls.client_cert_path = path.clone(),
                _ => mtls.client_key_path = path.clone(),
            }
            let error = build_mtls(&mtls).expect_err("invalid TLS material was accepted");
            assert!(matches!(
                error,
                TransportPreparationError::ReadFile { .. }
                    | TransportPreparationError::Pem { .. }
                    | TransportPreparationError::EmptyCertificates { .. }
                    | TransportPreparationError::Certificate { .. }
                    | TransportPreparationError::PrivateKeyCount
            ));
        }
    }
    let mut mtls = files.mtls();
    mtls.client_key_path = files.write(
        "invalid-key-der.pem",
        b"-----BEGIN PRIVATE KEY-----\nAQID\n-----END PRIVATE KEY-----\n",
    );
    assert!(matches!(
        build_mtls(&mtls),
        Err(TransportPreparationError::ClientIdentity(_))
    ));
}

#[test]
fn mismatched_keys_and_duplicate_keys_are_rejected() {
    let files = TlsFiles::new();
    let mut mtls = files.mtls();
    mtls.client_key_path = files.write("other-key.pem", SERVER_KEY);
    assert!(matches!(
        build_mtls(&mtls),
        Err(TransportPreparationError::ClientIdentity(
            rustls::Error::InconsistentKeys(_)
        ))
    ));
    mtls.client_key_path = files.write("duplicate-keys.pem", &[CLIENT_KEY, CLIENT_KEY].concat());
    assert!(matches!(
        build_mtls(&mtls),
        Err(TransportPreparationError::PrivateKeyCount)
    ));
}

#[test]
fn malformed_intermediate_certificate_is_rejected() {
    let files = TlsFiles::new();
    let mut mtls = files.mtls();
    mtls.client_cert_path = files.write(
        "bad-chain.pem",
        &[
            CLIENT_CERT,
            b"-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n",
        ]
        .concat(),
    );
    assert!(matches!(
        build_mtls(&mtls),
        Err(TransportPreparationError::Certificate { kind: "client", .. })
    ));
}

#[test]
fn duration_validation_rejects_zero_invalid_and_overflowing_values() {
    for value in [
        "",
        "invalid",
        "-1s",
        "0s",
        "0ms",
        "9223372036854775807s",
        "99999999999999999999999d",
    ] {
        assert!(matches!(
            connect_timeout("secure", Some(value), Duration::from_secs(5)),
            Err(TransportPreparationError::ConnectTimeout { .. })
        ));
    }
    assert_eq!(
        connect_timeout("secure", None, Duration::from_secs(5)).unwrap(),
        Duration::from_secs(5)
    );
    assert_eq!(
        connect_timeout("secure", Some("500ms"), Duration::from_secs(5)).unwrap(),
        Duration::from_millis(500)
    );
}

#[test]
fn durations_on_unused_upstreams_are_validated() {
    let config: gate::cfg::Config = serde_json::from_value(serde_json::json!({
        "schema": gate::cfg::SCHEMA,
        "ingress": {"limit": "ingress", "timeout": "250ms"},
        "limits": {"default": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "1s"}}, "ingress": {"strategy": "gcra", "params": {"max_burst": 100, "replenish_1_per": "100ms"}}},
        "http": { "upstreams": { "unused": { "targets": [{ "url": "http://localhost:8443" }], "transport": { "connect_timeout": "bad" } } } },
    })).unwrap();
    assert!(matches!(
        prepare(&RuntimeConfig::from_raw(config).unwrap()),
        Err(TransportPreparationError::ConnectTimeout { .. })
    ));
}

#[test]
fn unexpected_alpn_is_an_error_instead_of_a_connector_panic() {
    let mut tls = default_tls_config().unwrap();
    tls.alpn_protocols = vec![b"h2".to_vec()];
    assert!(matches!(
        build_hyper_client(Duration::from_secs(1), &tls, true, true),
        Err(TransportPreparationError::Alpn)
    ));
}

#[tokio::test]
async fn http_clients_enforce_configured_protocols_and_share_the_prepared_connection_settings() {
    use axum::body::Body;
    use hyper::{
        server::conn::{http1, http2},
        service::service_fn,
    };
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use std::convert::Infallible;
    for http2 in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(
                move |request: http::Request<hyper::body::Incoming>| async move {
                    assert_eq!(
                        request.version(),
                        if http2 {
                            http::Version::HTTP_2
                        } else {
                            http::Version::HTTP_11
                        }
                    );
                    Ok::<_, Infallible>(http::Response::new(Body::empty()))
                },
            );
            if http2 {
                http2::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(stream), service)
                    .await
                    .unwrap();
            } else {
                http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await
                    .unwrap();
            }
        });
        let files = TlsFiles::new();
        let mut config = files.config(&url);
        config.http.upstreams.get_mut("secure").unwrap().transport = Some(
            serde_json::from_value(serde_json::json!({
                "connect_timeout": "250ms",
                "protocols": [if http2 {"http2"} else {"http1"}]
            }))
            .unwrap(),
        );
        let prepared = prepare(&RuntimeConfig::from_raw(config).unwrap()).unwrap();
        let transport = prepared.get("secure").unwrap();
        assert_eq!(transport.http1, !http2);
        assert_eq!(transport.connect_timeout, Duration::from_millis(250));
        assert_eq!(
            transport.websocket_tls.alpn_protocols,
            vec![b"http/1.1".to_vec()]
        );
        let request = http::Request::builder()
            .uri(&url)
            .body(Body::empty())
            .unwrap();
        let response =
            tokio::time::timeout(Duration::from_secs(1), transport.http.request(request))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(
            response.version(),
            if http2 {
                http::Version::HTTP_2
            } else {
                http::Version::HTTP_11
            }
        );
        drop(response);
        drop(prepared);
        // A pooled client may retain an idle connection independently of the
        // request future; protocol assertions have already run on the server.
        server.abort();
    }
}

#[tokio::test]
async fn http_probes_use_http2_for_http2_only_upstreams_with_and_without_tls() {
    async fn serve<I>(stream: I)
    where
        I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let service = hyper::service::service_fn(
            |request: http::Request<hyper::body::Incoming>| async move {
                assert_eq!(request.version(), http::Version::HTTP_2);
                assert_eq!(request.uri().path(), "/health");
                Ok::<_, std::convert::Infallible>(http::Response::new(axum::body::Body::empty()))
            },
        );
        hyper::server::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
            .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
            .await
            .unwrap();
    }

    for use_tls in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "{}://{}",
            if use_tls { "https" } else { "http" },
            listener.local_addr().unwrap()
        );
        let acceptor = use_tls.then(|| {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let mut roots = rustls::RootCertStore::empty();
            for cert in rustls_pemfile::certs(&mut &CA[..]) {
                roots.add(cert.unwrap()).unwrap();
            }
            let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
                Arc::new(roots),
                provider.clone(),
            )
            .build()
            .unwrap();
            let certs = rustls_pemfile::certs(&mut &SERVER_CERT[..])
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let key = rustls_pemfile::private_key(&mut &SERVER_KEY[..])
                .unwrap()
                .unwrap();
            let mut tls = rustls::ServerConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_client_cert_verifier(verifier)
                .with_single_cert(certs, key)
                .unwrap();
            tls.alpn_protocols = vec![b"h2".to_vec()];
            tokio_rustls::TlsAcceptor::from(Arc::new(tls))
        });
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            if let Some(acceptor) = acceptor {
                let stream = acceptor.accept(stream).await.unwrap();
                assert_eq!(stream.get_ref().1.alpn_protocol(), Some(b"h2".as_slice()));
                serve(stream).await;
            } else {
                serve(stream).await;
            }
        });
        let files = TlsFiles::new();
        let mut raw = serde_json::to_value(files.config(&url)).unwrap();
        raw["http"]["upstreams"]["secure"]["transport"] =
            serde_json::json!({"protocols":["http2"]});
        raw["http"]["upstreams"]["secure"]["load_balancer"] =
            serde_json::json!({"strategy":"round_robin", "liveness_probe":{"path":"/health"}});
        let config = RuntimeConfig::from_raw(serde_json::from_value(raw).unwrap()).unwrap();
        let prepared = prepare(&config).unwrap();
        let client = prepared
            .get("secure")
            .unwrap()
            .probe_client
            .as_ref()
            .unwrap();
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            client.get(format!("{url}/health")).send(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.version(), http::Version::HTTP_2);
        server.abort();
    }
}

#[tokio::test]
async fn prepared_http_and_grpc_probes_honor_configured_timeouts() {
    for protocol in ["http", "grpc"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let files = TlsFiles::new();
        let mut raw = serde_json::to_value(files.config(&url)).unwrap();
        let mut probe = serde_json::json!({"protocol":protocol, "timeout":"50ms"});
        if protocol == "http" {
            probe["path"] = "/health".into();
        }
        raw["http"]["upstreams"]["secure"]["load_balancer"] =
            serde_json::json!({"strategy":"round_robin", "liveness_probe":probe});
        let config = RuntimeConfig::from_raw(serde_json::from_value(raw).unwrap()).unwrap();
        let prepared = prepare(&config).unwrap();
        let client = prepared
            .get("secure")
            .unwrap()
            .probe_client
            .as_ref()
            .unwrap();
        let error = tokio::time::timeout(
            Duration::from_secs(1),
            client.get(format!("{url}/health")).send(),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.is_timeout(), "{protocol}: {error}");
        server.abort();
    }
}
