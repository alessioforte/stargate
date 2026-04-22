mod act;
mod aud;
mod cli;
mod db;
mod err;
mod etc;
mod fun;
#[cfg(feature = "hyper-stack")]
mod gtw;
#[cfg(feature = "hyper-stack")]
mod http;

// =^.^=
// 🦀
// Stargate ✨
// 🚀

#[cfg(feature = "hyper-stack")]
mod boot_hyper {
    use crate::etc::{cors, gate, geoip, headers, jwt, log, logo, profile, store, tls};
    use crate::http::{api, mid};
    use crate::{act, aud, cli, db, gtw};
    use axum::Extension;
    use axum::middleware::from_fn;
    use dotenvy::dotenv;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tower::service_fn;
    use tower_http::{compression::CompressionLayer, normalize_path::NormalizePathLayer};
    use tracing::info;

    pub async fn run() -> std::io::Result<()> {
        println!("{}", logo::LOGO);

        dotenv().ok();
        tls::install_crypto_provider();
        let _guard = log::init();

        let cli = cli::parse();
        if let Some(command) = cli.command {
            return cli::run_cli_command(command).await;
        }

        aud::init();

        let version = env!("CARGO_PKG_VERSION");
        let port = std::env::var("PORT").unwrap_or_else(|_| "5050".to_string());
        let addr: SocketAddr = format!("0.0.0.0:{}", port)
            .parse()
            .expect("Invalid listen address");
        let tls_enabled = tls::enabled()?;
        let runtime_profile = profile::validate_runtime_profile()?;
        let shutdown_timeout_secs = act::server_shutdown_timeout_secs();

        info!("Version: {}", version);
        info!("Starting server on port {}", port);
        info!(
            "Runtime profile: {} (db={}, state={})",
            runtime_profile,
            profile::COMPILED_DB_BACKEND,
            profile::COMPILED_STATE_BACKEND
        );
        info!("Graceful shutdown timeout: {}s", shutdown_timeout_secs);

        jwt::init();
        geoip::init();
        store::init().await;
        if let Err(e) = db::init().await {
            tracing::error!("Database initialization failed: {}", e);
            std::process::exit(1);
        }

        let gate = gate::init();

        let app = api::router()
            .layer(from_fn(mid::rate_limit_middleware))
            .fallback_service(service_fn(gtw::service))
            .layer(Extension(Arc::clone(&gate)))
            .layer(from_fn(headers::security_headers_middleware))
            .layer(from_fn(log::trace_middleware))
            .layer(cors::hyper_configure())
            .layer(CompressionLayer::new())
            .layer(NormalizePathLayer::trim_trailing_slash());

        let shutdown = act::shutdown_signal()?;

        info!("Server listening on {}", addr);

        let result = if tls_enabled {
            run_tls(app, addr, shutdown, shutdown_timeout_secs).await
        } else {
            run_plain(app, addr, shutdown).await
        };

        info!("Server stopped, running shutdown hooks...");
        store::save().await;
        aud::shutdown().await;
        info!("Shutdown complete");

        result
    }

    async fn run_plain<F>(app: axum::Router, addr: SocketAddr, shutdown: F) -> std::io::Result<()>
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

    async fn run_tls<F>(
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
}

#[cfg(feature = "hyper-stack")]
#[tokio::main]
async fn main() -> std::io::Result<()> {
    boot_hyper::run().await
}
