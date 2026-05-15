mod act;
mod api;
mod aud;
mod cli;
mod db;
mod err;
mod etc;
mod fun;

use crate::etc::{cors, gate, geoip, headers, jwt, log, logo, profile, run, store, tls};

use axum::Extension;
use axum::middleware::from_fn;
use dotenvy::dotenv;
use std::net::SocketAddr;
use std::sync::Arc;
use tower::service_fn;
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
    geoip::init();
    store::init().await;
    if let Err(e) = db::init().await {
        tracing::error!("Database initialization failed: {}", e);
        std::process::exit(1);
    }

    let gate = gate::init();

    let app = api::router()
        .layer(from_fn(etc::mid::rate_limit_middleware))
        .fallback_service(service_fn(api::gateway::service))
        .layer(Extension(Arc::clone(&gate)))
        .layer(from_fn(headers::security_headers_middleware))
        .layer(from_fn(log::trace_middleware))
        .layer(cors::hyper_configure())
        .layer(CompressionLayer::new())
        .layer(NormalizePathLayer::trim_trailing_slash());

    let shutdown = fun::shutdown_signal()?;

    info!("Server listening on {}", addr);

    let result = if tls_enabled {
        run::tls(app, addr, shutdown, shutdown_timeout_secs).await
    } else {
        run::plain(app, addr, shutdown).await
    };

    info!("Server stopped, running shutdown hooks...");
    store::save().await;
    aud::shutdown().await;
    info!("Shutdown complete");

    result
}
