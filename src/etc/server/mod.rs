pub mod banner;
pub mod health;
pub mod profile;
pub mod tls;

use axum::extract::connect_info::ConnectInfo;
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use std::{net::SocketAddr, time::Duration};
use tokio::{net::TcpListener, sync::watch, task::JoinSet};
use tokio_rustls::TlsAcceptor;
use tracing::info;

pub async fn plain<F>(
    app: axum::Router,
    addr: SocketAddr,
    shutdown: F,
    timeout_secs: u64,
) -> std::io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind(addr).await?;
    serve(app, listener, shutdown, timeout_secs, None).await
}

pub async fn tls<F>(
    app: axum::Router,
    addr: SocketAddr,
    shutdown: F,
    timeout_secs: u64,
) -> std::io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let acceptor = TlsAcceptor::from(tls::rustls_server_config()?);
    let listener = TcpListener::bind(addr).await?;
    serve(app, listener, shutdown, timeout_secs, Some(acceptor)).await
}

async fn serve<F>(
    app: axum::Router,
    listener: TcpListener,
    shutdown: F,
    timeout_secs: u64,
    acceptor: Option<TlsAcceptor>,
) -> std::io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut tasks = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        while tasks.try_join_next().is_some() {}
        tokio::select! {
            biased;
            _ = &mut shutdown => {
                let _ = shutdown_tx.send(true);
                break;
            }
            accepted = listener.accept() => {
                let (tcp, peer) = match accepted {
                    Ok(connection) => connection,
                    Err(error) => { tracing::error!(%error, "Accept failed"); continue; }
                };
                let app = app.clone().layer(axum::Extension(ConnectInfo(peer)));
                let acceptor = acceptor.clone();
                let mut rx = shutdown_rx.clone();
                tasks.spawn(async move {
                    if let Some(acceptor) = acceptor {
                        let tls = tokio::select! {
                            biased;
                            _ = rx.changed() => return,
                            tls = acceptor.accept(tcp) => match tls { Ok(tls) => tls, Err(_) => return },
                        };
                        connection(tls, app, rx).await;
                    } else {
                        connection(tcp, app, rx).await;
                    }
                });
            }
        }
    }
    drop(listener);
    let drain = async { while tasks.join_next().await.is_some() {} };
    if tokio::time::timeout(Duration::from_secs(timeout_secs), drain)
        .await
        .is_err()
    {
        info!("Shutdown timeout reached, forcing close");
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
    Ok(())
}

async fn connection<I>(io: I, app: axum::Router, mut shutdown: watch::Receiver<bool>)
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let builder = Builder::new(TokioExecutor::new());
    let connection =
        builder.serve_connection_with_upgrades(TokioIo::new(io), TowerToHyperService::new(app));
    tokio::pin!(connection);
    let mut draining = *shutdown.borrow();
    if draining {
        connection.as_mut().graceful_shutdown();
    }
    loop {
        tokio::select! {
            result = &mut connection => {
                if let Err(error) = result { tracing::debug!(%error, "Connection closed"); }
                return;
            }
            _ = shutdown.changed(), if !draining => {
                draining = true;
                connection.as_mut().graceful_shutdown();
            }
        }
    }
}

#[cfg(all(test, feature = "memory"))]
mod tests;
