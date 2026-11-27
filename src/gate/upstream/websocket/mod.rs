mod handshake;
mod relay;

use super::UpstreamRequest;
use super::headers::UpstreamHeaders;
use crate::gate::lifecycle::{Execution, body::TransferTimer};
use crate::{
    err::{ErrorCode, ErrorResponse},
    gate::PreparedTransport,
};
use axum::{body::Body, response::Response};
use gate::graph::WebSocketLimits;
use handshake::{build_upstream_request, connect_upstream, validate_upgrade};
use http::header::SEC_WEBSOCKET_PROTOCOL;
use relay::proxy_websocket;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::protocol::WebSocketConfig};

type UpstreamSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn handler(
    runtime: std::sync::Arc<crate::gate::RuntimeSnapshot>,
    transport: &PreparedTransport,
    mut req: http::Request<Body>,
    upstream: UpstreamRequest<'_>,
    ingress: &UpstreamHeaders,
    limits: &WebSocketLimits,
    execution: &Execution,
) -> Result<Response, ErrorResponse> {
    validate_upgrade(&req)?;
    if !transport.http1() {
        return Err(
            ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed)
                .with_param("reason", "websocket_requires_http1"),
        );
    }

    let config = web_socket_config(limits);
    let (mut response, websocket) = hyper_tungstenite::upgrade(&mut req, Some(config))
        .map_err(|_| ErrorResponse::new(ErrorCode::GatewayWebsocketUpgradeInvalid))?;

    let upstream_request = build_upstream_request(upstream, req.headers(), ingress).await?;
    let (upstream_ws, upstream_response) = connect_upstream(
        upstream_request,
        transport,
        execution,
        runtime.settings.response_header_timeout,
        config,
    )
    .await?;

    if let Some(protocol) = upstream_response.headers().get(SEC_WEBSOCKET_PROTOCOL) {
        response
            .headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, protocol.clone());
    }

    // Internal context authenticates only the completed HTTP handshake. The
    // accepted connection then proxies frames without reminting or replacing
    // the identity for its lifetime.
    let admission = req
        .extensions()
        .get::<std::sync::Arc<crate::gate::resources::Admission>>()
        .cloned();
    let mut execution = execution.clone();
    execution.stream = true;
    let resources = runtime.resources.clone();
    let spawned = resources.spawn(async move {
        let mut timer = TransferTimer::new("websocket", execution.cancellation.clone());
        let result = proxy_websocket(
            websocket,
            upstream_ws,
            &execution,
            &runtime.resources,
            runtime.settings.response_body_idle_timeout,
            runtime.settings.response_header_timeout,
        )
        .await;
        timer.finish(if execution.cancellation.is_cancelled() {
            "cancelled"
        } else if execution.failure().is_some() {
            "timeout"
        } else if result.is_err() {
            "error"
        } else {
            "complete"
        });
        if let Err(error) = result {
            tracing::warn!(%error, "WebSocket proxy closed with error");
        }
        drop(admission);
        drop(runtime);
    });
    if !spawned {
        return Err(ErrorResponse::new(ErrorCode::GatewayCancelled));
    }

    Ok(response.map(Body::new))
}

/// The same limits apply to the client and the upstream socket: a peer cannot
/// make the gateway buffer a larger message on either side of the relay.
fn web_socket_config(limits: &WebSocketLimits) -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(limits.max_message_bytes))
        .max_frame_size(Some(limits.max_frame_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_limits_apply_to_both_socket_sizes() {
        let config = web_socket_config(&WebSocketLimits {
            max_message_bytes: 4096,
            max_frame_bytes: 1024,
        });

        assert_eq!(config.max_message_size, Some(4096));
        assert_eq!(config.max_frame_size, Some(1024));
        assert!(!config.accept_unmasked_frames);
    }

    #[test]
    fn default_limits_are_explicit() {
        let config = web_socket_config(&WebSocketLimits::default());

        assert_eq!(config.max_message_size, Some(8 * 1024 * 1024));
        assert_eq!(config.max_frame_size, Some(1024 * 1024));
    }
}

#[cfg(test)]
mod test_support;
