mod act;
mod api;
mod aud;
mod cli;
mod db;
mod err;
mod etc;
mod fun;

use crate::etc::{
    cors, gate, geoip, headers, internal_context, jwt, log, logo, profile, run, store, tls,
};

use axum::Extension;
use axum::middleware::from_fn;
use dotenvy::dotenv;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::{compression::CompressionLayer, normalize_path::NormalizePathLayer};
use tracing::info;

// =^.^=
// 🦀
// Stargate ✨
// 🚀
#[tokio::main]
async fn main() -> std::io::Result<()> {
    run().await
}

pub async fn run() -> std::io::Result<()> {
    println!("{}", logo::LOGO);

    dotenv().ok();
    tls::install_crypto_provider();

    let cli = cli::parse();
    if let Some(command) = cli.command {
        if command.is_standalone() {
            return cli::run_cli_command(command).await;
        }
        let guard = log::init();
        etc::pw::init();
        let result = cli::run_cli_command(command).await;
        guard.shutdown();
        return result;
    }

    let guard = log::init();
    etc::pw::init();

    let version = env!("CARGO_PKG_VERSION");
    let port = std::env::var("PORT").unwrap_or_else(|_| "5050".to_string());
    let addr: SocketAddr = format!("0.0.0.0:{}", port)
        .parse()
        .expect("Invalid listen address");
    let tls_enabled = tls::enabled()?;
    let runtime_profile = profile::validate_runtime_profile()?;
    let shutdown_timeout_secs = fun::server_shutdown_timeout_secs();

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
    internal_context::init().map_err(std::io::Error::other)?;
    act::password_policy::init();
    geoip::init();
    store::init().await;
    if let Err(e) = db::init().await {
        tracing::error!("Database initialization failed: {}", e);
        std::process::exit(1);
    }

    aud::spawn()?;

    let gate = gate::init().map_err(std::io::Error::other)?;
    let lifecycle = etc::health::Lifecycle::default();

    let app = api::server_router(lifecycle.clone())
        .layer(Extension(Arc::clone(&gate)))
        .layer(from_fn(headers::security_headers_middleware))
        .layer(from_fn(log::trace_middleware))
        .layer(cors::hyper_configure())
        .layer(CompressionLayer::new())
        .layer(NormalizePathLayer::trim_trailing_slash());

    let signal = fun::shutdown_signal()?;

    lifecycle.mark_ready();
    let gateway_resources = gate.resources.clone();
    let shutdown = async move {
        fun::drain_on_shutdown(
            signal,
            lifecycle,
            std::time::Duration::from_secs(fun::server_drain_delay_secs()),
        )
        .await;
        gateway_resources.begin_shutdown();
    };

    info!("Server listening on {}", addr);

    let result = if tls_enabled {
        run::tls(app, addr, shutdown, shutdown_timeout_secs).await
    } else {
        run::plain(app, addr, shutdown, shutdown_timeout_secs).await
    };

    info!("Server stopped, running shutdown hooks...");
    gate.resources.begin_shutdown();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(shutdown_timeout_secs),
        gate.resources.wait(),
    )
    .await;
    aud::shutdown().await;
    store::save().await;
    info!("Shutdown complete");
    guard.shutdown();

    result
}
