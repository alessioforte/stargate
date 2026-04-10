mod act;
mod api;
mod aud;
mod cli;
mod db;
mod err;
mod etc;
mod fun;
mod gtw;

use crate::etc::{cfg, cors, gate, geoip, headers, jwt, log, logo, profile, store, tls};
use actix_web::{
    App, HttpServer,
    middleware::{self, TrailingSlash},
};
use dotenvy::dotenv;

use tracing::info;
use tracing_actix_web::TracingLogger;

// =^.^=
// 🦀
// Stargate ✨
// 🚀

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("{}", logo::LOGO);

    dotenv().ok();
    let _guard = log::init();

    let cli = cli::parse();
    if let Some(command) = cli.command {
        return cli::run_cli_command(command).await;
    }

    aud::init();

    let version = env!("CARGO_PKG_VERSION");
    let port = std::env::var("PORT").unwrap_or_else(|_| "5050".to_string());
    let addrs = format!("0.0.0.0:{}", port);
    let tls_enabled = tls::enabled()?;
    let runtime_profile = profile::validate_runtime_profile()?;

    info!("Version: {}", version);
    info!("Starting server on port {}", port);
    info!(
        "Runtime profile: {} (db={}, state={})",
        runtime_profile,
        profile::COMPILED_DB_BACKEND,
        profile::COMPILED_STATE_BACKEND
    );

    jwt::init();
    geoip::init();
    store::init().await;
    if let Err(e) = db::init().await {
        tracing::error!("Database initialization failed: {}", e);
        std::process::exit(1);
    }

    let gcfg = gate::init();

    let server = HttpServer::new(move || {
        App::new()
            .app_data(gcfg.clone())
            .wrap(middleware::NormalizePath::new(TrailingSlash::Trim))
            .wrap(middleware::Compress::default())
            .wrap(headers::configure())
            .wrap(TracingLogger::<log::StargateRootSpanBuilder>::new())
            .wrap(cors::configure())
            .configure(cfg::configure)
            .configure(api::configure)
            .configure(gtw::configure)
    });

    let result = if tls_enabled {
        let tls = tls::builder()?;
        server.bind_openssl(addrs, tls)?.run().await
    } else {
        server.bind(addrs)?.run().await
    };

    info!("Server stopped, running shutdown hooks...");
    store::save().await;
    aud::shutdown().await;
    info!("Shutdown complete");

    result
}
