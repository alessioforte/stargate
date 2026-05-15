use tracing::info;

const DEFAULT_SERVER_SHUTDOWN_TIMEOUT_SECS: u64 = 25;

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
