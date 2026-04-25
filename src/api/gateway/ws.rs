use crate::err::{ErrorResponse, HttpError};
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
) -> Result<Response, ErrorResponse> {
    if !hyper_tungstenite::is_upgrade_request(&req) {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Invalid websocket upgrade request".to_string(),
        )));
    }

    let upstream_request = build_upstream_request(uri, req.headers(), preserve_host)?;
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
) -> Result<http::Request<()>, ErrorResponse> {
    let mut request = uri.into_client_request().map_err(|error| {
        tracing::error!(%uri, %error, "Invalid WebSocket upstream URI");
        ErrorResponse::from(HttpError::BadGateway(
            "Failed to connect to backend websocket".to_string(),
        ))
    })?;

    copy_forwarded_headers(request.headers_mut(), incoming_headers, preserve_host);

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
    use super::{build_upstream_request, copy_forwarded_headers, should_forward_request_header};
    use http::header::{
        AUTHORIZATION, CONNECTION, COOKIE, HOST, ORIGIN, SEC_WEBSOCKET_EXTENSIONS,
        SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_PROTOCOL, SEC_WEBSOCKET_VERSION, UPGRADE,
    };

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
            build_upstream_request("wss://upstream.example/socket?foo=1", &headers, false).unwrap();

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
}
