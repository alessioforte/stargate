use super::UpstreamSocket;
use crate::err::{ErrorCode, ErrorResponse};
use crate::gate::{lifecycle::Execution, resources::ProcessResources};
use futures_util::{SinkExt, StreamExt};
use hyper_tungstenite::HyperWebsocket;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::Instant;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Error, Message,
        error::ProtocolError,
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

const WEBSOCKET_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
type RelayError = Box<dyn std::error::Error + Send + Sync>;
type RelayResult = Result<(), RelayError>;

#[derive(Clone, Copy)]
enum Peer {
    Client,
    Backend,
}

enum RelayEnd {
    Cancelled,
    /// The Close reason: the idle timeout or the router's request deadline.
    Timeout(&'static str),
    Closed(Peer, Option<CloseFrame>),
    Lost(Peer, Error),
    Error(ErrorResponse),
}

enum Closing {
    Send(Option<CloseFrame>),
    Reply,
}

pub(super) async fn proxy_websocket(
    websocket: HyperWebsocket,
    mut upstream: UpstreamSocket,
    execution: &Execution,
    resources: &ProcessResources,
    idle: Duration,
    handshake: Duration,
) -> RelayResult {
    let downstream = execution
        .run("websocket_handshake", Some(handshake), websocket)
        .await
        .map_err(RelayError::from)
        .and_then(|result| result.map_err(RelayError::from));
    let mut downstream = match downstream {
        Ok(socket) => socket,
        Err(error) => {
            // The backend is already upgraded even if the client's upgrade fails.
            let reason = if execution.cancellation.is_cancelled() {
                "gateway shutting down"
            } else {
                ""
            };
            finish_side(
                &mut upstream,
                Some(Closing::Send(Some(close_frame(1001, reason)))),
                resources,
                "backend",
                Instant::now() + WEBSOCKET_CLOSE_TIMEOUT,
            )
            .await;
            return Err(error);
        }
    };
    relay_sockets(
        &mut downstream,
        &mut upstream,
        &execution.response(),
        resources,
        idle,
    )
    .await
}

async fn relay_sockets<D, U>(
    downstream: &mut WebSocketStream<D>,
    upstream: &mut WebSocketStream<U>,
    execution: &Execution,
    resources: &ProcessResources,
    idle: Duration,
) -> RelayResult
where
    D: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    let end = relay_loop(downstream, upstream, execution, idle).await;
    let going_away = |reason| {
        let frame = close_frame(1001, reason);
        (
            Some(Closing::Send(Some(frame.clone()))),
            Some(Closing::Send(Some(frame))),
        )
    };
    let (client_close, backend_close) = match &end {
        RelayEnd::Cancelled => going_away("gateway shutting down"),
        RelayEnd::Timeout(reason) => going_away(reason),
        RelayEnd::Error(_) => going_away(""),
        RelayEnd::Closed(Peer::Client, frame) => {
            (Some(Closing::Reply), Some(Closing::Send(frame.clone())))
        }
        RelayEnd::Closed(Peer::Backend, frame) => {
            (Some(Closing::Send(frame.clone())), Some(Closing::Reply))
        }
        // 1011, not the IANA-registered 1014 (Bad Gateway): tungstenite 0.30
        // rejects 1014 on receipt and reports a protocol violation instead.
        RelayEnd::Lost(Peer::Backend, _) => (
            Some(Closing::Send(Some(close_frame(
                1011,
                "upstream connection lost",
            )))),
            None,
        ),
        RelayEnd::Lost(Peer::Client, _) => (None, Some(Closing::Send(Some(close_frame(1001, ""))))),
    };
    // Execution is already cancelled or timed out. Closing must run independently,
    // with both peers sharing the same five-second/global shutdown ceiling.
    let deadline = Instant::now() + WEBSOCKET_CLOSE_TIMEOUT;
    tokio::join!(
        finish_side(downstream, client_close, resources, "client", deadline),
        finish_side(upstream, backend_close, resources, "backend", deadline),
    );
    match end {
        RelayEnd::Lost(_, error) => Err(error.into()),
        RelayEnd::Error(error) => Err(error.into()),
        _ => Ok(()),
    }
}

async fn relay_loop<D, U>(
    downstream: &mut WebSocketStream<D>,
    upstream: &mut WebSocketStream<U>,
    execution: &Execution,
    idle: Duration,
) -> RelayEnd
where
    D: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        // tokio-tungstenite 0.30 queues the complete serialized frame before I/O;
        // tungstenite's FrameCodec retains the unwritten suffix on WouldBlock.
        // Cancelling send therefore preserves bytes for the following Close to
        // flush in order (see cancelled_send_finishes_the_frame_before_close).
        let result = execution.run("websocket_idle", Some(idle), async {
            tokio::select! {
                inbound = downstream.next() => match inbound {
                    Some(Ok(Message::Close(frame))) => Some(RelayEnd::Closed(Peer::Client, frame)),
                    Some(Ok(message)) => upstream.send(message).await.err().map(|error| RelayEnd::Lost(Peer::Backend, error)),
                    Some(Err(error)) => Some(RelayEnd::Lost(Peer::Client, error)),
                    None => Some(RelayEnd::Lost(Peer::Client, Error::Protocol(ProtocolError::ResetWithoutClosingHandshake))),
                },
                outbound = upstream.next() => match outbound {
                    Some(Ok(Message::Close(frame))) => Some(RelayEnd::Closed(Peer::Backend, frame)),
                    Some(Ok(message)) => downstream.send(message).await.err().map(|error| RelayEnd::Lost(Peer::Client, error)),
                    Some(Err(error)) => Some(RelayEnd::Lost(Peer::Backend, error)),
                    None => Some(RelayEnd::Lost(Peer::Backend, Error::Protocol(ProtocolError::ResetWithoutClosingHandshake))),
                },
            }
        }).await;
        match result {
            Ok(Some(end)) => return end,
            Ok(None) => {}
            Err(error) if error.code == ErrorCode::GatewayCancelled => return RelayEnd::Cancelled,
            Err(error) if error.code == ErrorCode::GatewayTimeout => {
                let idle = error.params.get("phase").and_then(|phase| phase.as_str())
                    == Some("websocket_idle");
                return RelayEnd::Timeout(if idle {
                    "idle timeout"
                } else {
                    "request timeout"
                });
            }
            Err(error) => return RelayEnd::Error(error),
        }
    }
}

fn close_frame(code: u16, reason: &'static str) -> CloseFrame {
    CloseFrame {
        code: CloseCode::from(code),
        reason: reason.into(),
    }
}

async fn finish_side<S>(
    socket: &mut WebSocketStream<S>,
    closing: Option<Closing>,
    resources: &ProcessResources,
    peer: &'static str,
    deadline: Instant,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let Some(closing) = closing else {
        return;
    };
    let close = close_socket(socket, closing);
    tokio::pin!(close);
    let result = tokio::time::timeout_at(deadline, async {
        tokio::select! {
            biased;
            _ = resources.shutdown.cancelled() => {
                // Shutdown can begin while an idle/peer-initiated close is already
                // in progress. Shorten its ceiling without restarting the exchange.
                let global = resources.shutdown_deadline().unwrap_or(deadline);
                tokio::time::timeout_at(deadline.min(global), &mut close).await
            }
            result = &mut close => Ok(result),
        }
    })
    .await;
    match result {
        Err(_) | Ok(Err(_)) => tracing::debug!(peer, "WebSocket close handshake timed out"),
        Ok(Ok(Err(Error::ConnectionClosed | Error::AlreadyClosed))) | Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(error))) => {
            tracing::debug!(peer, %error, "WebSocket peer disconnected during close")
        }
    }
}

async fn close_socket<S>(socket: &mut WebSocketStream<S>, closing: Closing) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match closing {
        Closing::Reply => socket.flush().await,
        Closing::Send(frame) => {
            socket.send(Message::Close(frame)).await?;
            loop {
                match socket.next().await {
                    Some(Ok(Message::Close(_))) => break socket.flush().await,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => break Err(error),
                    None => break Ok(()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
