mod api;
mod aud;
mod cli;
mod conf;
mod ctx;
mod data;
mod err;
mod gate;
mod iam;
mod obs;
mod srv;
mod web;

use crate::{
    ctx::geoip,
    data::store,
    iam::auth::jwt,
    obs::logging,
    srv::{banner, profile, shutdown, tls},
    web::{cors, headers},
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
    dotenv().ok();
    tls::install_crypto_provider();

    let cli = cli::parse();
    if let Some(command) = cli.command {
        // Configuration command output is data; tracing and banners must not enter stdout.
        if matches!(&command, cli::Command::Config(_)) {
            return cli::run_cli_command(command).await;
        }
        println!("{}", banner::LOGO);
        if command.is_standalone() {
            return cli::run_cli_command(command).await;
        }
        let guard = logging::init(logging::Settings::from_env()?);
        iam::pwd::hash::init();
        let result = cli::run_cli_command(command).await;
        guard.shutdown();
        return result;
    }

    let log_settings = logging::Settings::from_env()?;
    if log_settings.format() != logging::LogFormat::Json {
        println!("{}", banner::LOGO);
    }
    let guard = logging::init(log_settings);
    iam::pwd::hash::init();

    let version = env!("CARGO_PKG_VERSION");
    let port = std::env::var("PORT").unwrap_or_else(|_| "5050".to_string());
    let addr: SocketAddr = format!("0.0.0.0:{}", port)
        .parse()
        .expect("Invalid listen address");
    let tls_enabled = tls::enabled()?;
    let runtime_profile = profile::validate_runtime_profile()?;
    let shutdown_timeout_secs = shutdown::server_shutdown_timeout_secs();

    info!("Version: {}", version);
    info!("Starting server on port {}", port);
    info!(
        "Runtime profile: {} (db={}, state={})",
        runtime_profile,
        profile::COMPILED_DB_BACKEND,
        profile::COMPILED_STATE_BACKEND
    );
    info!("Graceful shutdown timeout: {}s", shutdown_timeout_secs);

    conf::features::init().map_err(|error| {
        tracing::error!("Invalid feature settings: {error}");
        std::io::Error::other(error.to_string())
    })?;
    conf::hosts::init().map_err(|error| {
        tracing::error!("Invalid IAM host settings: {error}");
        std::io::Error::other(error)
    })?;
    iam::otp::settings::init().map_err(|error| {
        tracing::error!("Invalid MFA settings: {error}");
        std::io::Error::other(error.to_string())
    })?;
    jwt::init();
    ctx::sign::init().map_err(std::io::Error::other)?;
    iam::pwd::policy::init();
    geoip::init();
    #[cfg(feature = "memory")]
    store::init().await;
    #[cfg(feature = "redis")]
    store::init().await.map_err(std::io::Error::other)?;
    if let Err(e) = data::db::init().await {
        tracing::error!("Database initialization failed: {}", e);
        std::process::exit(1);
    }

    aud::spawn()?;

    let gate = gate::init().await.map_err(std::io::Error::other)?;
    gate::spawn_config_sync(gate.clone())?;
    let lifecycle = srv::health::Lifecycle::default();

    let app = api::server_router(lifecycle.clone(), conf::iam_hosts().clone())
        .layer(Extension(Arc::clone(&gate)))
        .layer(from_fn(headers::security_headers_middleware))
        .layer(from_fn(logging::trace_middleware))
        .layer(cors::hyper_configure())
        .layer(CompressionLayer::new())
        .layer(NormalizePathLayer::trim_trailing_slash());

    let signal = shutdown::shutdown_signal()?;

    lifecycle.mark_ready();
    let gateway_resources = gate.resources.clone();
    let shutdown = async move {
        shutdown::drain_on_shutdown(
            signal,
            lifecycle,
            std::time::Duration::from_secs(shutdown::server_drain_delay_secs()),
        )
        .await;
        gateway_resources.begin_shutdown();
    };

    info!("Server listening on {}", addr);

    let result = if tls_enabled {
        srv::tls(app, addr, shutdown, shutdown_timeout_secs).await
    } else {
        srv::plain(app, addr, shutdown, shutdown_timeout_secs).await
    };

    info!("Server stopped, running shutdown hooks...");
    gate.resources.begin_shutdown();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(shutdown_timeout_secs),
        gate.resources.wait(),
    )
    .await;
    gate::shutdown_config_sync().await;
    aud::shutdown().await;
    store::save().await;
    info!("Shutdown complete");
    guard.shutdown();

    result
}
