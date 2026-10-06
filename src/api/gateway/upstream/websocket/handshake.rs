use super::UpstreamSocket;
use crate::api::gateway::{
    lifecycle::Execution,
    upstream::{headers::strip_hop_by_hop_headers, internal_context::InternalDispatch},
};
use crate::{
    err::{ErrorCode, ErrorResponse},
    etc::{gate::PreparedTransport, observability::telemetry},
};
use axum::body::Body;
use base64::Engine as _;
use http::header::{
    CONTENT_LENGTH, HOST, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_EXTENSIONS, SEC_WEBSOCKET_KEY,
    SEC_WEBSOCKET_PROTOCOL, SEC_WEBSOCKET_VERSION,
};
use tokio_tungstenite::{MaybeTlsStream, client_async, tungstenite::client::IntoClientRequest};

pub(super) fn validate_upgrade(req: &http::Request<Body>) -> Result<(), ErrorResponse> {
    let invalid = || ErrorResponse::new(ErrorCode::GatewayWebsocketUpgradeInvalid);
    // RFC 6455 section 4.2.1; extended CONNECT is not an HTTP/1 Upgrade.
    if req.method() != http::Method::GET
        || req.version() != http::Version::HTTP_11
        || !hyper_tungstenite::is_upgrade_request(req)
    {
        return Err(invalid());
    }
    for name in [HOST, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION] {
        if req.headers().get_all(&name).iter().count() != 1 {
            return Err(invalid());
        }
    }
    let host = req.headers()[HOST].to_str().map_err(|_| invalid())?;
    if host.parse::<http::uri::Authority>().is_err() || req.headers()[SEC_WEBSOCKET_VERSION] != "13"
    {
        return Err(invalid());
    }
    let key = base64::engine::general_purpose::STANDARD
        .decode(req.headers()[SEC_WEBSOCKET_KEY].as_bytes())
        .map_err(|_| invalid())?;
    if key.len() != 16 {
        return Err(invalid());
    }
    Ok(())
}

pub(super) async fn connect_upstream(
    request: http::Request<()>,
    transport: &PreparedTransport,
    execution: &Execution,
    handshake_timeout: std::time::Duration,
) -> Result<(UpstreamSocket, http::Response<Option<Vec<u8>>>), ErrorResponse> {
    let invalid = || ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed);
    let host = request
        .uri()
        .host()
        .ok_or_else(invalid)?
        .trim_matches(['[', ']'])
        .to_owned();
    let tls = match request.uri().scheme_str() {
        Some("wss") => {
            Some(rustls::pki_types::ServerName::try_from(host.clone()).map_err(|_| invalid())?)
        }
        Some("ws") => None,
        _ => return Err(invalid()),
    };
    let port = request
        .uri()
        .port_u16()
        .unwrap_or(if tls.is_some() { 443 } else { 80 });
    // Connect bounds DNS, TCP, and TLS together. The Upgrade response has its
    // own header deadline, with both phases capped by the execution deadline.
    let stream = execution
        .run("connect", transport.connect_timeout, async {
            let socket = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
            match tls {
                Some(server_name) => {
                    // RFC 6455 section 4.1: TLS identity comes from the target URI,
                    // independently of a preserved application Host field.
                    let connector =
                        tokio_rustls::TlsConnector::from(transport.websocket_tls.clone());
                    Ok::<_, std::io::Error>(MaybeTlsStream::Rustls(
                        connector.connect(server_name, socket).await?,
                    ))
                }
                None => Ok(MaybeTlsStream::Plain(socket)),
            }
        })
        .await?
        .map_err(|error| {
            tracing::error!(%error, "WebSocket upstream connection failed");
            ErrorResponse::new(ErrorCode::UpstreamConnectionFailed)
        })?;
    let (socket, response) = execution
        .run(
            "websocket_handshake",
            handshake_timeout,
            client_async(request, stream),
        )
        .await?
        .map_err(|error| {
            tracing::error!(%error, "WebSocket upstream handshake failed");
            ErrorResponse::new(ErrorCode::UpstreamConnectionFailed)
        })?;
    // We never offer extensions: tungstenite does not implement their framing.
    // RFC 6455 section 4.1 requires rejecting unsolicited extensions.
    if response.headers().contains_key(SEC_WEBSOCKET_EXTENSIONS)
        || response
            .headers()
            .get_all(SEC_WEBSOCKET_PROTOCOL)
            .iter()
            .count()
            > 1
    {
        return Err(ErrorResponse::new(ErrorCode::UpstreamConnectionFailed));
    }
    Ok((socket, response))
}

pub(super) async fn build_upstream_request(
    uri: &str,
    incoming_headers: &http::HeaderMap,
    preserve_host: bool,
    internal_dispatch: Option<&InternalDispatch>,
) -> Result<http::Request<()>, ErrorResponse> {
    let mut request = uri.into_client_request().map_err(|error| {
        tracing::error!(%uri, %error, "Invalid WebSocket upstream URI");
        ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed)
    })?;

    copy_forwarded_headers(request.headers_mut(), incoming_headers, preserve_host)?;
    if let Some(dispatch) = internal_dispatch {
        dispatch.prepare(&mut request).await?;
    } else {
        telemetry::inject_context(request.headers_mut());
    }

    Ok(request)
}

fn copy_forwarded_headers(
    target: &mut http::HeaderMap,
    source: &http::HeaderMap,
    preserve_host: bool,
) -> Result<(), ErrorResponse> {
    let mut source = source.clone();
    strip_hop_by_hop_headers(&mut source);
    if preserve_host {
        let mut hosts = source.get_all(HOST).iter();
        let host = hosts
            .next()
            .ok_or_else(|| ErrorResponse::new(ErrorCode::GatewayWebsocketUpgradeInvalid))?;
        if hosts.next().is_some() {
            return Err(ErrorResponse::new(
                ErrorCode::GatewayWebsocketUpgradeInvalid,
            ));
        }
        // Replace the generated Host; append would serialize the target's value.
        target.insert(HOST, host.clone());
    }
    let protocols = source
        .get_all(SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .map(|value| value.to_str())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ErrorResponse::new(ErrorCode::GatewayWebsocketUpgradeInvalid))?;
    if !protocols.is_empty() {
        target.insert(
            SEC_WEBSOCKET_PROTOCOL,
            protocols
                .join(", ")
                .parse()
                .map_err(|_| ErrorResponse::new(ErrorCode::GatewayWebsocketUpgradeInvalid))?,
        );
    }
    for (name, value) in &source {
        if should_forward_request_header(name) {
            target.append(name, value.clone());
        }
    }
    Ok(())
}

fn should_forward_request_header(name: &http::HeaderName) -> bool {
    name != CONTENT_LENGTH
        && name != HOST
        && name != SEC_WEBSOCKET_ACCEPT
        && name != SEC_WEBSOCKET_EXTENSIONS
        && name != SEC_WEBSOCKET_KEY
        && name != SEC_WEBSOCKET_VERSION
        && name != SEC_WEBSOCKET_PROTOCOL
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tls_tests;
#[cfg(test)]
mod transport_tests;
