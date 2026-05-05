use super::tls;
use std::net::SocketAddr;
use tracing::info;

pub async fn plain<F>(app: axum::Router, addr: SocketAddr, shutdown: F) -> std::io::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}

pub async fn tls<F>(
    app: axum::Router,
    addr: SocketAddr,
    shutdown: F,
    timeout_secs: u64,
) -> std::io::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use hyper_util::server::conn::auto::Builder as ConnBuilder;
    use hyper_util::service::TowerToHyperService;
    use tokio::net::TcpListener;
    use tokio::sync::watch;
    use tokio::task::JoinSet;
    use tokio_rustls::TlsAcceptor;
    use tower::Service;

    let tls_config = tls::rustls_server_config()?;
    let acceptor = TlsAcceptor::from(tls_config);
    let listener = TcpListener::bind(addr).await?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut join_set: JoinSet<()> = JoinSet::new();
    let mut shutdown = std::pin::pin!(shutdown);

    info!("TLS enabled, listening on {}", addr);

    loop {
        // reap completed tasks without blocking
        while join_set.try_join_next().is_some() {}

        tokio::select! {
            res = listener.accept() => {
                let (tcp, peer_addr) = match res {
                    Ok(v) => v,
                    Err(e) => { tracing::error!("accept error: {e}"); continue; }
                };
                let acceptor = acceptor.clone();
                let mut ms = app.clone().into_make_service_with_connect_info::<SocketAddr>();
                let mut rx = shutdown_rx.clone();
                join_set.spawn(async move {
                    let Ok(tls) = acceptor.accept(tcp).await else { return; };
                    let svc = match ms.call(peer_addr).await {
                        Ok(svc) => svc,
                        Err(never) => match never {},
                    };
                    let io = TokioIo::new(tls);
                    let builder = ConnBuilder::new(TokioExecutor::new());
                    let conn = builder
                        .serve_connection_with_upgrades(io, TowerToHyperService::new(svc));
                    tokio::pin!(conn);
                    let mut shutdown_signaled = false;
                    loop {
                        tokio::select! {
                            res = conn.as_mut() => {
                                if let Err(e) = res {
                                    tracing::debug!("connection error: {e}");
                                }
                                break;
                            }
                            _ = rx.changed(), if !shutdown_signaled => {
                                shutdown_signaled = true;
                                conn.as_mut().graceful_shutdown();
                            }
                        }
                    }
                });
            }
            _ = &mut shutdown => {
                let _ = shutdown_tx.send(true);
                break;
            }
        }
    }

    // drain connections up to timeout
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        if join_set.is_empty() {
            break;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            info!("Shutdown timeout reached, forcing close");
            join_set.abort_all();
            break;
        }
        tokio::select! {
            Some(_) = join_set.join_next() => {}
            _ = tokio::time::sleep(remaining) => {
                info!("Shutdown timeout reached, forcing close");
                join_set.abort_all();
                break;
            }
        }
    }

    Ok(())
}
