use super::UpstreamSocket;
use crate::api::gateway::lifecycle::Execution;
use futures_util::{SinkExt, StreamExt};
use hyper_tungstenite::HyperWebsocket;

pub(super) async fn proxy_websocket(
    websocket: HyperWebsocket,
    mut upstream: UpstreamSocket,
    execution: &Execution,
    idle: std::time::Duration,
    handshake: std::time::Duration,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut downstream = execution
        .run("websocket_handshake", handshake, websocket)
        .await??;
    let execution = execution.response();
    loop {
        // Both receipt and forwarding must make progress; a blocked sink also expires.
        let should_close = execution.run("websocket_idle", idle, async {
            tokio::select! {
                inbound = downstream.next() => {
                    let Some(message) = inbound else { return Ok::<_, tokio_tungstenite::tungstenite::Error>(true); };
                    let message = message?;
                    let close = message.is_close();
                    upstream.send(message).await?;
                    if close { downstream.flush().await?; }
                    Ok(close)
                }
                outbound = upstream.next() => {
                    let Some(message) = outbound else { return Ok::<_, tokio_tungstenite::tungstenite::Error>(true); };
                    let message = message?;
                    let close = message.is_close();
                    downstream.send(message).await?;
                    if close { upstream.flush().await?; }
                    Ok(close)
                }
            }
        }).await??;
        if should_close {
            return Ok(());
        }
    }
}
