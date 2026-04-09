mod act;
mod api;
mod aud;
mod db;
mod err;
mod etc;
mod fun;
mod gtw;

use crate::etc::{cfg, cors, gate, geoip, headers, jwt, log, logo, store, tls};
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
    aud::init();

    let version = env!("CARGO_PKG_VERSION");
    let port = std::env::var("PORT").unwrap_or_else(|_| "5050".to_string());
    let addrs = format!("0.0.0.0:{}", port);
    let tls_enabled = tls::enabled()?;

    info!("Version: {}", version);
    info!("Starting server on port {}", port);

    jwt::init();
    geoip::init();
    store::init().await;
    if let Err(e) = db::init().await {
        tracing::error!("Database initialization failed: {}", e);
        std::process::exit(1);
    }

    let gcfg = gate::init();

    fun::create_super_admin().await;

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
