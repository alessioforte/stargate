use super::*;
use crate::etc::gate::{RuntimeSnapshot, test_support};
use gate::cfg::{Config, MtlsConfig, RuntimeConfig};
use http::header::{CONNECTION, ORIGIN, UPGRADE};
use std::{sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::handshake::server::{Request as ServerRequest, Response as ServerResponse},
};

pub(super) fn runtime(
    url: &str,
    protocols: &[&str],
    timeout: &str,
    mtls: Option<MtlsConfig>,
) -> Arc<RuntimeSnapshot> {
    let mut value = serde_json::to_value(Config::default()).unwrap();
    value["mtls"] = serde_json::to_value(mtls).unwrap();
    value["http"] = serde_json::json!({
        "upstreams":{"socket":{"targets":[{"url":url}],"transport":{"protocols":protocols,"connect_timeout":timeout}}},
        "services":{"socket":{"kind":"load_balancer","upstream":"socket"}}
    });
    test_support::runtime(RuntimeConfig::from_raw(serde_json::from_value(value).unwrap()).unwrap())
}

pub(super) fn execution(runtime: &RuntimeSnapshot) -> Execution {
    Execution::new(
        Duration::from_secs(5),
        runtime.resources.shutdown.child_token(),
    )
}

#[test]
fn serialized_host_is_replaced_once_and_connection_nominated_headers_are_removed() {
    let mut source = http::HeaderMap::new();
    source.insert(HOST, "gateway.example:8443".parse().unwrap());
    source.append(CONNECTION, "Upgrade, x-private, origin".parse().unwrap());
    source.append(CONNECTION, "x-another, sec-websocket-key".parse().unwrap());
    for name in [
        "x-private",
        "x-another",
        "keep-alive",
        "proxy-connection",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "content-length",
    ] {
        source.insert(name, "must-not-forward".parse().unwrap());
    }
    source.insert(ORIGIN, "https://removed.example".parse().unwrap());
    source.insert(
        SEC_WEBSOCKET_KEY,
        "client-key-must-be-replaced".parse().unwrap(),
    );
    source.append(SEC_WEBSOCKET_PROTOCOL, "chat".parse().unwrap());
    source.append(SEC_WEBSOCKET_PROTOCOL, "superchat".parse().unwrap());
    for preserve in [false, true] {
        let request =
            build_upstream_request("ws://backend.example:8080/socket", &source, preserve, None)
                .unwrap();
        let expected = if preserve {
            "gateway.example:8443"
        } else {
            "backend.example:8080"
        };
        assert_eq!(request.headers()[HOST], expected);
        assert_eq!(request.headers().get_all(HOST).iter().count(), 1);
        assert_eq!(request.headers()[SEC_WEBSOCKET_PROTOCOL], "chat, superchat");
        assert_eq!(request.headers()[CONNECTION], "Upgrade");
        assert_eq!(request.headers()[UPGRADE], "websocket");
        assert_ne!(
            request.headers()[SEC_WEBSOCKET_KEY],
            source[SEC_WEBSOCKET_KEY]
        );
        for name in [
            "x-private",
            "x-another",
            "origin",
            "keep-alive",
            "proxy-connection",
            "proxy-authorization",
            "te",
            "trailer",
            "transfer-encoding",
            "content-length",
        ] {
            assert!(!request.headers().contains_key(name), "{name} leaked");
        }
        let (bytes, _) =
            tokio_tungstenite::tungstenite::handshake::client::generate_request(request).unwrap();
        let serialized = String::from_utf8(bytes).unwrap();
        let hosts = serialized
            .lines()
            .filter(|line| line.starts_with("Host:"))
            .collect::<Vec<_>>();
        assert_eq!(hosts, vec![format!("Host: {expected}")]);
    }
    source.append(HOST, "ambiguous.example".parse().unwrap());
    assert!(build_upstream_request("ws://backend.example/socket", &source, true, None).is_err());
}

#[tokio::test]
async fn unsupported_transport_and_invalid_client_handshakes_fail_before_connecting() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/socket", listener.local_addr().unwrap());
    let runtime = runtime(&url, &["http2"], "1s", None);
    let valid = url
        .as_str()
        .into_client_request()
        .unwrap()
        .map(|_| Body::empty());
    let error = handler(
        runtime.clone(),
        runtime.transport("socket").unwrap(),
        valid,
        &url,
        false,
        None,
        &execution(&runtime),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::GatewayRequestPreparationFailed);
    assert_eq!(error.params["reason"], "websocket_requires_http1");
    for invalid in [
        "method",
        "http10",
        "http2",
        "host",
        "duplicate_host",
        "key",
        "duplicate_key",
        "version",
        "duplicate_version",
        "extended_connect",
    ] {
        let mut req = url
            .as_str()
            .into_client_request()
            .unwrap()
            .map(|_| Body::empty());
        match invalid {
            "method" => *req.method_mut() = http::Method::POST,
            "http10" => *req.version_mut() = http::Version::HTTP_10,
            "http2" => *req.version_mut() = http::Version::HTTP_2,
            "host" => {
                req.headers_mut().remove(HOST);
            }
            "duplicate_host" => {
                req.headers_mut()
                    .append(HOST, "other.example".parse().unwrap());
            }
            "key" => {
                req.headers_mut()
                    .insert(SEC_WEBSOCKET_KEY, "invalid".parse().unwrap());
            }
            "duplicate_key" => {
                let key = req.headers()[SEC_WEBSOCKET_KEY].clone();
                req.headers_mut().append(SEC_WEBSOCKET_KEY, key);
            }
            "version" => {
                req.headers_mut()
                    .insert(SEC_WEBSOCKET_VERSION, "12".parse().unwrap());
            }
            "duplicate_version" => {
                req.headers_mut()
                    .append(SEC_WEBSOCKET_VERSION, "13".parse().unwrap());
            }
            "extended_connect" => {
                *req.method_mut() = http::Method::CONNECT;
                *req.version_mut() = http::Version::HTTP_2;
                req.headers_mut().clear();
                req.extensions_mut()
                    .insert(hyper::ext::Protocol::from_static("websocket"));
                use crate::etc::ext::RequestExt;
                assert_eq!(req.get_protocol(), "ws");
            }
            _ => unreachable!(),
        }
        let error = handler(
            runtime.clone(),
            runtime.transport("socket").unwrap(),
            req,
            &url,
            false,
            None,
            &execution(&runtime),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.code,
            ErrorCode::GatewayWebsocketUpgradeInvalid,
            "{invalid}"
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn tls_connect_and_upgrade_have_separate_deadlines_and_use_the_upstream_override() {
    for tls in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let url = format!(
            "{}://localhost:{}/socket",
            if tls { "wss" } else { "ws" },
            address.port()
        );
        let runtime = runtime(&url, &[], "30ms", None);
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let request = build_upstream_request(&url, &http::HeaderMap::new(), false, None).unwrap();
        let started = std::time::Instant::now();
        let error = connect_upstream(
            request,
            runtime.transport("socket").unwrap(),
            &execution(&runtime),
            Duration::from_millis(80),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::GatewayTimeout);
        assert_eq!(
            error.params["phase"],
            if tls {
                "connect"
            } else {
                "websocket_handshake"
            }
        );
        assert!(started.elapsed() < Duration::from_millis(500));
        server.abort();
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/socket", listener.local_addr().unwrap());
    let runtime = runtime(&url, &[], "30ms", None);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        tokio_tungstenite::accept_async(stream).await.unwrap()
    });
    let request = build_upstream_request(&url, &http::HeaderMap::new(), false, None).unwrap();
    let (socket, _) = connect_upstream(
        request,
        runtime.transport("socket").unwrap(),
        &execution(&runtime),
        Duration::from_millis(300),
    )
    .await
    .unwrap();
    drop(socket);
    drop(server.await.unwrap());
}

#[tokio::test]
async fn upstream_extensions_and_duplicate_subprotocol_selection_are_rejected() {
    for extension in [true, false] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/socket", listener.local_addr().unwrap());
        let runtime = runtime(&url, &[], "1s", None);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            #[allow(clippy::result_large_err)]
            let callback = move |_: &ServerRequest, mut response: ServerResponse| {
                if extension {
                    response.headers_mut().insert(
                        SEC_WEBSOCKET_EXTENSIONS,
                        "permessage-deflate".parse().unwrap(),
                    );
                } else {
                    response
                        .headers_mut()
                        .append(SEC_WEBSOCKET_PROTOCOL, "chat".parse().unwrap());
                    response
                        .headers_mut()
                        .append(SEC_WEBSOCKET_PROTOCOL, "chat".parse().unwrap());
                }
                Ok(response)
            };
            accept_hdr_async(stream, callback).await.unwrap()
        });
        let mut incoming = http::HeaderMap::new();
        incoming.insert(SEC_WEBSOCKET_PROTOCOL, "chat".parse().unwrap());
        let request = build_upstream_request(&url, &incoming, false, None).unwrap();
        let error = connect_upstream(
            request,
            runtime.transport("socket").unwrap(),
            &execution(&runtime),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::UpstreamConnectionFailed);
        drop(server.await.unwrap());
    }
}
