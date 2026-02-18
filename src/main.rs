mod act;
mod api;
mod aud;
mod db;
mod err;
mod etc;
mod fun;
mod gtw;

use crate::etc::{cfg, cors, gate, jwt, log, logo, store, tls};
use actix_web::{
    App, HttpServer,
    middleware::{self, TrailingSlash},
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
    store::init().await;
    db::init().await;
    let gcfg = gate::init();

    fun::create_super_admin().await;

    let server = HttpServer::new(move || {
        App::new()
            .app_data(gcfg.clone())
            .wrap(middleware::NormalizePath::new(TrailingSlash::Trim))
            .wrap(middleware::Compress::default())
            .wrap(TracingLogger::<log::StargateRootSpanBuilder>::new())
            .wrap(cors::middleware::configure())
            .configure(cfg::configure)
            .configure(api::configure)
            .configure(gtw::configure)
    });

    if tls_enabled == "true" {
        let tls = tls::builder();
        return server.bind_openssl(addrs, tls)?.run().await;
    }

    server.bind(addrs)?.run().await
}
