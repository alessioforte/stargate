mod handshake;
mod relay;

use super::internal_context::InternalDispatch;
use crate::api::gateway::lifecycle::{Execution, body::TransferTimer};
use crate::{
    err::{ErrorCode, ErrorResponse},
    etc::gate::PreparedTransport,
};
use axum::{body::Body, response::Response};
use handshake::{build_upstream_request, connect_upstream, validate_upgrade};
use http::header::SEC_WEBSOCKET_PROTOCOL;
use relay::proxy_websocket;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type UpstreamSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn handler(
    runtime: std::sync::Arc<crate::etc::gate::RuntimeSnapshot>,
    transport: &PreparedTransport,
    mut req: http::Request<Body>,
    uri: &str,
    preserve_host: bool,
    internal_dispatch: Option<&InternalDispatch>,
    execution: &Execution,
) -> Result<Response, ErrorResponse> {
    validate_upgrade(&req)?;
    if !transport.http1 {
        return Err(
            ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed)
                .with_param("reason", "websocket_requires_http1"),
        );
    }

    let (mut response, websocket) = hyper_tungstenite::upgrade(&mut req, None)
        .map_err(|_| ErrorResponse::new(ErrorCode::GatewayWebsocketUpgradeInvalid))?;

    let upstream_request =
        build_upstream_request(uri, req.headers(), preserve_host, internal_dispatch).await?;
    let (upstream_ws, upstream_response) = connect_upstream(
        upstream_request,
        transport,
        execution,
        runtime.settings.response_header_timeout,
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
        .get::<std::sync::Arc<crate::etc::gate::resources::Admission>>()
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

#[cfg(test)]
mod test_support;
