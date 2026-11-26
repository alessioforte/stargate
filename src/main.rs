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
    srv::{banner, profile, settings, shutdown, tls},
    web::headers,
};

use axum::Extension;
use axum::middleware::from_fn;
use dotenvy::dotenv;
use std::sync::Arc;
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

    // Every `?` below passes through the guard shutdown, so startup errors
    // reach the file sink and the OTLP log exporter instead of staying in a
    // batch that the process exit discards.
    let result = serve().await;
    guard.shutdown();
    result
}

/// The server after logging is initialized. The caller shuts the log guard
/// down on every outcome, normal shutdown and startup error alike.
async fn serve() -> std::io::Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let listeners = settings::from_env().map_err(|error| {
        tracing::error!("Invalid listener settings: {error}");
        std::io::Error::other(error.to_string())
    })?;
    if !settings::serves_probes(&listeners) {
        tracing::warn!(
            "No listener declares the management plane: `/livez` and `/readyz` answer nowhere; declare a listener with `LISTENER_<NAME>_PLANES=management` to expose the probes"
        );
    }
    let runtime_profile = profile::validate_runtime_profile()?;
    let shutdown_timeout_secs = shutdown::server_shutdown_timeout_secs();

    info!("Version: {}", version);
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
    web::cors::init().map_err(|error| {
        tracing::error!("Invalid CORS settings: {error}");
        std::io::Error::other(error)
    })?;
    web::origin::init().map_err(|error| {
        tracing::error!("Invalid trusted origin settings: {error}");
        std::io::Error::other(error)
    })?;
    iam::oidc::http_client::init().map_err(|error| {
        tracing::error!("Invalid OAuth client endpoint settings: {error}");
        std::io::Error::other(error)
    })?;
    #[cfg(feature = "memory")]
    conf::data::init().map_err(|error| {
        tracing::error!("Invalid edge data directory settings: {error}");
        std::io::Error::other(error.to_string())
    })?;
    #[cfg(feature = "memory")]
    store::init_settings().map_err(|error| {
        tracing::error!("Invalid memory store settings: {error}");
        std::io::Error::other(error.to_string())
    })?;
    iam::otp::settings::init().map_err(|error| {
        tracing::error!("Invalid MFA settings: {error}");
        std::io::Error::other(error.to_string())
    })?;
    smtp::settings::init().map_err(|error| {
        tracing::error!("Invalid SMTP settings: {error}");
        std::io::Error::other(error.to_string())
    })?;
    smtp::validate_templates().map_err(|error| {
        tracing::error!("Invalid email templates: {error}");
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
    data::db::init().await.map_err(|error| {
        tracing::error!("Database initialization failed: {error:#}");
        std::io::Error::other(format!("{error:#}"))
    })?;

    aud::spawn()?;

    let gate = gate::init().await.map_err(std::io::Error::other)?;
    gate::spawn_config_sync(gate.clone())?;
    let lifecycle = srv::health::Lifecycle::default();

    // One router per listener: each serves only the planes it declares, with the
    // server-wide layers on top. The gateway extension and the layers are
    // applied once here, never per connection.
    let hosts = conf::iam_hosts().clone();
    let listeners = listeners
        .into_iter()
        .map(|settings| {
            let router = api::listener_router(settings.planes, lifecycle.clone(), hosts.clone())
                .layer(Extension(Arc::clone(&gate)))
                .layer(from_fn(headers::security_headers_middleware))
                .layer(from_fn(logging::trace_middleware));
            srv::Listener { settings, router }
        })
        .collect::<Vec<_>>();
    let tls_config = listeners
        .iter()
        .any(|listener| listener.settings.tls)
        .then(tls::rustls_server_config)
        .transpose()?;

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
        gateway_resources
            .begin_shutdown_with_timeout(std::time::Duration::from_secs(shutdown_timeout_secs));
    };

    let result = srv::serve(
        listeners,
        tls_config,
        shutdown,
        std::time::Duration::from_secs(shutdown_timeout_secs),
    )
    .await;

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

    result
}
