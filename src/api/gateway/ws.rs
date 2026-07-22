use super::dispatch::InternalDispatch;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::telemetry;
use axum::{body::Body, response::Response};
use futures_util::{SinkExt, StreamExt};
use http::header::{
    CONNECTION, CONTENT_LENGTH, HOST, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_EXTENSIONS,
    SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_PROTOCOL, SEC_WEBSOCKET_VERSION, TE, TRAILER,
    TRANSFER_ENCODING, UPGRADE,
};
use hyper_tungstenite::HyperWebsocket;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{self, client::IntoClientRequest},
};

pub async fn handler(
    mut req: http::Request<Body>,
    uri: &str,
    preserve_host: bool,
    internal_dispatch: Option<&InternalDispatch>,
) -> Result<Response, ErrorResponse> {
    if !hyper_tungstenite::is_upgrade_request(&req) {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Invalid websocket upgrade request".to_string(),
        )));
    }

    let upstream_request =
        build_upstream_request(uri, req.headers(), preserve_host, internal_dispatch)?;
    let (upstream_ws, upstream_response) =
        connect_async(upstream_request).await.map_err(|error| {
            tracing::error!(%uri, %error, "WebSocket upstream connect failed");
            ErrorResponse::from(HttpError::BadGateway(
                "Failed to connect to backend websocket".to_string(),
            ))
        })?;

    let (mut response, websocket) =
        hyper_tungstenite::upgrade(&mut req, None).map_err(|error| {
            tracing::error!(%error, "WebSocket upgrade failed");
            ErrorResponse::from(HttpError::BadRequest(
                "Invalid websocket upgrade request".to_string(),
            ))
        })?;

    if let Some(protocol) = upstream_response.headers().get(SEC_WEBSOCKET_PROTOCOL) {
        response
            .headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, protocol.clone());
    }

    // Internal context authenticates only the completed HTTP handshake. The
    // accepted connection then proxies frames without reminting or replacing
    // the identity for its lifetime.
    tokio::spawn(async move {
        if let Err(error) = proxy_websocket(websocket, upstream_ws).await {
            tracing::warn!(%error, "WebSocket proxy closed with error");
        }
    });

    Ok(response.map(Body::new))
}

fn build_upstream_request(
    uri: &str,
    incoming_headers: &http::HeaderMap,
    preserve_host: bool,
    internal_dispatch: Option<&InternalDispatch>,
) -> Result<http::Request<()>, ErrorResponse> {
    let mut request = uri.into_client_request().map_err(|error| {
        tracing::error!(%uri, %error, "Invalid WebSocket upstream URI");
        ErrorResponse::from(HttpError::BadGateway(
            "Failed to connect to backend websocket".to_string(),
        ))
    })?;

    copy_forwarded_headers(request.headers_mut(), incoming_headers, preserve_host);
    if let Some(dispatch) = internal_dispatch {
        dispatch.prepare(&mut request)?;
    } else {
        telemetry::inject_context(request.headers_mut());
    }

    Ok(request)
}

fn copy_forwarded_headers(
    target: &mut http::HeaderMap,
    source: &http::HeaderMap,
    preserve_host: bool,
) {
    for (name, value) in source {
        if should_forward_request_header(name, preserve_host) {
            target.append(name, value.clone());
        }
    }
}

fn should_forward_request_header(name: &http::HeaderName, preserve_host: bool) -> bool {
    name != CONNECTION
        && name != CONTENT_LENGTH
        && (preserve_host || name != HOST)
        && name != SEC_WEBSOCKET_ACCEPT
        && name != SEC_WEBSOCKET_EXTENSIONS
        && name != SEC_WEBSOCKET_KEY
        && name != SEC_WEBSOCKET_VERSION
        && name != TE
        && name != TRAILER
        && name != TRANSFER_ENCODING
        && name != UPGRADE
}

async fn proxy_websocket(
    websocket: HyperWebsocket,
    mut upstream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Result<(), tungstenite::Error> {
    let mut downstream = match websocket.await {
        Ok(websocket) => websocket,
        Err(error) => {
            let _ = upstream.close(None).await;
            return Err(error);
        }
    };

    loop {
        tokio::select! {
            inbound = downstream.next() => {
                match inbound {
                    Some(Ok(message)) => {
                        let should_close = message.is_close();
                        upstream.send(message).await?;
                        if should_close {
                            break;
                        }
                    }
                    Some(Err(error)) => return Err(error),
                    None => {
                        let _ = upstream.close(None).await;
                        break;
                    }
                }
            }
            outbound = upstream.next() => {
                match outbound {
                    Some(Ok(message)) => {
                        let should_close = message.is_close();
                        downstream.send(message).await?;
                        if should_close {
                            break;
                        }
                    }
                    Some(Err(error)) => return Err(error),
                    None => {
                        let _ = downstream.close(None).await;
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etc::{
        guard::VerifiedIdentity,
        internal_context::InternalContextRuntime,
        reqctx::{INTERNAL_CONTEXT_HEADER, PropagationDraft, RequestContext},
    };
    use chrono::Utc;
    use ctx::{
        ContextSigner, ContextVerifier, DispatchKind, ExpectedRequest, SignerConfig,
        StaticKeyResolver, VerifierConfig,
    };
    use http::header::{
        AUTHORIZATION, CONNECTION, COOKIE, HOST, ORIGIN, SEC_WEBSOCKET_EXTENSIONS,
        SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_PROTOCOL, SEC_WEBSOCKET_VERSION, UPGRADE,
    };
    use std::sync::Arc;
    use tokio::sync::oneshot;
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::{
            Message,
            handshake::server::{Request as ServerRequest, Response as ServerResponse},
        },
    };

    const ISSUER: &str = "https://stargate.test/internal-context";
    const AUDIENCE: &str = "urn:stargate:service:socket";
    const KEY_ID: &str = "stargate-internal-test";
    const REQUEST_ID: &str = "01JZ000000000000000000000R";
    const PRIVATE_KEY: &[u8] = include_bytes!("../../../crates/ctx/tests/fixtures/private.pem");
    const PUBLIC_KEY: &[u8] = include_bytes!("../../../crates/ctx/tests/fixtures/public.pem");

    fn internal_dispatch() -> InternalDispatch {
        let signer = ContextSigner::from_rsa_pem(
            SignerConfig::new(ISSUER, KEY_ID, 30).unwrap(),
            PRIVATE_KEY,
        )
        .unwrap();
        let runtime = InternalContextRuntime::from_signer_for_test(signer);
        let request = RequestContext::new(REQUEST_ID.to_owned(), Utc::now(), None, None, None);
        let draft = Arc::new(
            PropagationDraft::build(
                &VerifiedIdentity::anonymous(),
                &request,
                "GET",
                "/socket",
                "/socket",
                "socket",
                "socket",
                None,
            )
            .unwrap(),
        );
        InternalDispatch::new(
            "orders",
            AUDIENCE,
            Some(&draft),
            Some(&runtime),
            DispatchKind::Primary,
            1,
        )
        .unwrap()
    }

    #[test]
    fn forwards_request_headers_needed_by_upstream() {
        let mut source = http::HeaderMap::new();
        source.insert(CONNECTION, "upgrade".parse().unwrap());
        source.insert(UPGRADE, "websocket".parse().unwrap());
        source.insert(HOST, "gateway.local".parse().unwrap());
        source.insert(SEC_WEBSOCKET_KEY, "abc123".parse().unwrap());
        source.insert(SEC_WEBSOCKET_VERSION, "13".parse().unwrap());
        source.insert(
            SEC_WEBSOCKET_EXTENSIONS,
            "permessage-deflate".parse().unwrap(),
        );
        source.insert(SEC_WEBSOCKET_PROTOCOL, "chat, superchat".parse().unwrap());
        source.insert(AUTHORIZATION, "Bearer token".parse().unwrap());
        source.insert(COOKIE, "jwt=abc".parse().unwrap());
        source.insert(ORIGIN, "https://client.local".parse().unwrap());
        source.insert("x-request-id", "req_123".parse().unwrap());

        let mut target = http::HeaderMap::new();
        copy_forwarded_headers(&mut target, &source, false);

        assert!(target.get(CONNECTION).is_none());
        assert!(target.get(UPGRADE).is_none());
        assert!(target.get(HOST).is_none());
        assert!(target.get(SEC_WEBSOCKET_KEY).is_none());
        assert!(target.get(SEC_WEBSOCKET_VERSION).is_none());
        assert!(target.get(SEC_WEBSOCKET_EXTENSIONS).is_none());
        assert_eq!(
            target.get(SEC_WEBSOCKET_PROTOCOL).unwrap(),
            "chat, superchat"
        );
        assert_eq!(target.get(AUTHORIZATION).unwrap(), "Bearer token");
        assert_eq!(target.get(COOKIE).unwrap(), "jwt=abc");
        assert_eq!(target.get(ORIGIN).unwrap(), "https://client.local");
        assert_eq!(target.get("x-request-id").unwrap(), "req_123");
    }

    #[test]
    fn upstream_request_builds_from_ws_uri() {
        let mut headers = http::HeaderMap::new();
        headers.insert(SEC_WEBSOCKET_PROTOCOL, "chat".parse().unwrap());
        headers.insert("x-trace-id", "trace_1".parse().unwrap());

        let request =
            build_upstream_request("wss://upstream.example/socket?foo=1", &headers, false, None)
                .unwrap();

        assert_eq!(request.uri(), "wss://upstream.example/socket?foo=1");
        assert_eq!(
            request.headers().get(SEC_WEBSOCKET_PROTOCOL).unwrap(),
            "chat"
        );
        assert_eq!(request.headers().get("x-trace-id").unwrap(), "trace_1");
    }

    #[test]
    fn sec_websocket_protocol_is_forwardable() {
        assert!(should_forward_request_header(
            &SEC_WEBSOCKET_PROTOCOL,
            false
        ));
        assert!(!should_forward_request_header(&SEC_WEBSOCKET_KEY, false));
        assert!(!should_forward_request_header(&CONNECTION, false));
    }

    #[test]
    fn host_header_is_forwarded_when_preserved() {
        let mut source = http::HeaderMap::new();
        source.insert(HOST, "gateway.local".parse().unwrap());

        let mut target = http::HeaderMap::new();
        copy_forwarded_headers(&mut target, &source, true);

        assert_eq!(target.get(HOST).unwrap(), "gateway.local");
    }

    // `accept_hdr_async` fixes the callback error type to tungstenite's full HTTP
    // response; the test callback cannot make that dependency-owned type smaller.
    #[allow(clippy::result_large_err)]
    #[tokio::test]
    async fn internal_websocket_handshake_is_sanitized_verified_and_frame_compatible() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, captured) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let callback = move |request: &ServerRequest, mut response: ServerResponse| {
                if let Some(protocol) = request.headers().get(SEC_WEBSOCKET_PROTOCOL) {
                    response
                        .headers_mut()
                        .insert(SEC_WEBSOCKET_PROTOCOL, protocol.clone());
                }
                sender
                    .send((request.uri().clone(), request.headers().clone()))
                    .unwrap();
                Ok(response)
            };
            let mut socket = accept_hdr_async(stream, callback).await.unwrap();
            let message = socket.next().await.unwrap().unwrap();
            socket.send(message).await.unwrap();
        });

        let mut incoming = http::HeaderMap::new();
        incoming.insert(AUTHORIZATION, "Bearer edge-secret".parse().unwrap());
        incoming.insert("proxy-authorization", "Basic edge-secret".parse().unwrap());
        incoming.insert("x-api-key", "edge-api-key".parse().unwrap());
        incoming.insert("baggage", "private=value".parse().unwrap());
        incoming.insert("x-request-id", "attacker-id".parse().unwrap());
        incoming.insert("stargate-context", "attacker-context".parse().unwrap());
        incoming.insert(COOKIE, "theme=dark; jwt=edge-session".parse().unwrap());
        incoming.insert(ORIGIN, "https://client.test".parse().unwrap());
        incoming.insert(SEC_WEBSOCKET_PROTOCOL, "chat".parse().unwrap());
        let target = format!("ws://{address}/socket?%6awt=secret&safe=%2Fvalue");
        let request =
            build_upstream_request(&target, &incoming, false, Some(&internal_dispatch())).unwrap();

        let (mut client, _) = connect_async(request).await.unwrap();
        client.send(Message::Text("ping".into())).await.unwrap();
        assert_eq!(
            client.next().await.unwrap().unwrap(),
            Message::Text("ping".into())
        );
        let (uri, headers) = captured.await.unwrap();

        assert_eq!(uri.path(), "/socket");
        assert_eq!(uri.query(), Some("safe=%2Fvalue"));
        for name in [
            AUTHORIZATION,
            http::header::PROXY_AUTHORIZATION,
            http::HeaderName::from_static("x-api-key"),
            http::HeaderName::from_static("baggage"),
        ] {
            assert!(headers.get_all(name).iter().next().is_none());
        }
        assert_eq!(headers[COOKIE], "theme=dark");
        assert_eq!(headers[ORIGIN], "https://client.test");
        assert_eq!(headers[SEC_WEBSOCKET_PROTOCOL], "chat");
        assert!(headers.get(SEC_WEBSOCKET_KEY).is_some());
        assert_eq!(headers[SEC_WEBSOCKET_VERSION], "13");
        assert_eq!(headers.get_all("x-request-id").iter().count(), 1);
        assert_eq!(headers["x-request-id"], REQUEST_ID);

        let tokens = headers
            .get_all(INTERNAL_CONTEXT_HEADER)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(tokens.len(), 1);
        let resolver = StaticKeyResolver::from_rsa_pem(KEY_ID, PUBLIC_KEY).unwrap();
        let verifier = ContextVerifier::new(
            VerifierConfig::new(ISSUER, AUDIENCE, 5).unwrap(),
            Arc::new(resolver),
        );
        let trusted = verifier
            .verify(
                tokens[0],
                ExpectedRequest {
                    method: "GET",
                    encoded_path: "/socket",
                    request_id: REQUEST_ID,
                },
            )
            .unwrap();
        assert_eq!(trusted.context().dispatch.kind, DispatchKind::Primary);
        assert_eq!(trusted.context().dispatch.attempt, 1);

        client.close(None).await.unwrap();
        server.await.unwrap();
    }
}
