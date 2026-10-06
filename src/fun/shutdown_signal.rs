use tracing::info;

const DEFAULT_SERVER_SHUTDOWN_TIMEOUT_SECS: u64 = 25;
const DEFAULT_SERVER_DRAIN_DELAY_SECS: u64 = 5;

pub fn server_drain_delay_secs() -> u64 {
    std::env::var("SERVER_DRAIN_DELAY_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_SERVER_DRAIN_DELAY_SECS)
}

pub async fn drain_on_shutdown(
    signal: impl std::future::Future<Output = ()>,
    lifecycle: crate::etc::server::health::Lifecycle,
    delay: std::time::Duration,
) {
    signal.await;
    lifecycle.begin_shutdown();
    info!(
        delay_secs = delay.as_secs(),
        "Readiness disabled; waiting before closing listeners"
    );
    tokio::time::sleep(delay).await;
}

pub fn server_shutdown_timeout_secs() -> u64 {
    std::env::var("SERVER_SHUTDOWN_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|timeout: &u64| *timeout > 0)
        .unwrap_or(DEFAULT_SERVER_SHUTDOWN_TIMEOUT_SECS)
}

#[cfg(unix)]
pub fn shutdown_signal() -> std::io::Result<impl std::future::Future<Output = ()> + Send + 'static>
{
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    Ok(async move {
        tokio::select! {
            _ = sigterm.recv() => info!("SIGTERM received, starting graceful shutdown"),
            _ = sigint.recv() => info!("SIGINT received, starting graceful shutdown"),
        }
    })
}

#[cfg(not(unix))]
pub fn shutdown_signal() -> std::io::Result<impl std::future::Future<Output = ()> + Send + 'static>
{
    Ok(async move {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!("Failed to listen for Ctrl-C: {}", error);
            return;
        }

        info!("Ctrl-C received, starting graceful shutdown");
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etc::server::health::Lifecycle;
    use std::time::Duration;
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn shutdown_disables_readiness_before_the_listener_closes() {
        for delay in [Duration::ZERO, Duration::from_secs(5)] {
            let lifecycle = Lifecycle::default();
            lifecycle.mark_ready();
            let (signal_tx, signal_rx) = tokio::sync::oneshot::channel();
            let shutdown =
                drain_on_shutdown(async { signal_rx.await.unwrap() }, lifecycle.clone(), delay);
            tokio::pin!(shutdown);
            assert!(futures_util::poll!(&mut shutdown).is_pending());
            assert!(lifecycle.check_ready(async { true }).await);

            let started = Instant::now();
            signal_tx.send(()).unwrap();
            if !delay.is_zero() {
                assert!(futures_util::poll!(&mut shutdown).is_pending());
                assert!(!lifecycle.check_ready(async { true }).await);
            }
            shutdown.await;
            assert_eq!(started.elapsed(), delay);
            assert!(!lifecycle.check_ready(async { true }).await);
        }
    }
}
