mod act;
mod api;
mod aud;
mod db;
mod err;
mod etc;
mod fun;
mod gtw;

use crate::etc::{cfg, cors, gate, geoip, jwt, log, logo, store, tls};
use actix_web::{
    App, HttpServer,
    http::header,
    middleware::{self, DefaultHeaders, TrailingSlash},
};
use dotenvy::dotenv;
use std::env;
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
    let port = env::var("PORT").unwrap_or_else(|_| "5050".to_string());
    let addrs = format!("0.0.0.0:{}", port);
    let tls_enabled = env::var("TLS_ENABLED").unwrap_or_else(|_| "true".to_string());

    info!("Version: {}", version);
    info!("Starting server on port {}", port);

    jwt::init();
    geoip::init();
    store::init().await;
    db::init().await;

    let gcfg = gate::init();

    fun::create_super_admin().await;

    let server = HttpServer::new(move || {
        App::new()
            .app_data(gcfg.clone())
            .wrap(middleware::NormalizePath::new(TrailingSlash::Trim))
            .wrap(middleware::Compress::default())
            .wrap(
                DefaultHeaders::new()
                    .add((header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
                    .add((header::X_FRAME_OPTIONS, "DENY"))
                    .add((
                        header::STRICT_TRANSPORT_SECURITY,
                        "max-age=63072000; includeSubDomains",
                    )),
            )
            .wrap(TracingLogger::<log::StargateRootSpanBuilder>::new())
            .wrap(cors::middleware::configure())
            .configure(cfg::configure)
            .configure(api::configure)
            .configure(gtw::configure)
    });

    let result = if tls_enabled == "true" {
        let tls = tls::builder();
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
