use super::tls;
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
mod tests {
    use super::*;
    use axum::{body::Body, response::Response};
    use std::{
        convert::Infallible,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn plain_shutdown_deadline_drops_stalled_response_and_preserves_connect_info() {
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = dropped.clone();
        let app =
            axum::Router::new().fallback(move |ConnectInfo(peer): ConnectInfo<SocketAddr>| {
                let body_guard = Dropped(flag.clone());
                async move {
                    assert!(peer.ip().is_loopback());
                    let frames = futures_util::stream::unfold(body_guard, |guard| async move {
                        let _guard = guard;
                        std::future::pending::<
                            Option<(Result<hyper::body::Bytes, Infallible>, Dropped)>,
                        >()
                        .await
                    });
                    Response::new(Body::from_stream(frames))
                }
            });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (signal, shutdown) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve(
            app,
            listener,
            async {
                let _ = shutdown.await;
            },
            1,
            None,
        ));
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut headers = vec![0; 1024];
        let len = client.read(&mut headers).await.unwrap();
        assert!(String::from_utf8_lossy(&headers[..len]).starts_with("HTTP/1.1 200"));
        signal.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(client.read(&mut headers).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn shutdown_cancels_incomplete_inbound_tls_handshake() {
        let certs = rustls_pemfile::certs(&mut &crate::etc::gate::test_support::SERVER_CERT[..])
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let key = rustls_pemfile::private_key(&mut &crate::etc::gate::test_support::SERVER_KEY[..])
            .unwrap()
            .unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (signal, shutdown) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve(
            axum::Router::new(),
            listener,
            async {
                let _ = shutdown.await;
            },
            1,
            Some(TlsAcceptor::from(Arc::new(config))),
        ));
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(b"\x16\x03\x03\x00\x14\x01").await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        signal.send(()).unwrap();
        tokio::time::timeout(Duration::from_millis(500), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
