mod act;
mod aud;
mod db;
mod err;
mod etc;
mod fun;
mod gtw;
mod rts;

use crate::etc::{cfg, cors, ctx, gate, jwt, log, logo, store, tls};
use actix_web::{
    App, HttpServer,
    middleware::{self, TrailingSlash, from_fn},
};
use dotenvy::dotenv;
use std::env;
use tracing::info;

// =^.^=
// 🦀
// Stargate ✨
// 🚀

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("{}", logo::LOGO);

    dotenv().ok();
    log::init();
    aud::init();

    let version = env!("CARGO_PKG_VERSION");
    let port = env::var("PORT").unwrap_or_else(|_| "5050".to_string());
    let addrs = format!("0.0.0.0:{}", port);
    let tls_enabled = env::var("TLS_ENABLED").unwrap_or_else(|_| "true".to_string());

    info!("Version: {}", version);
    info!("Starting server on port {}", port);

    jwt::init();
    store::init();
    db::init().await;
    let gcfg = gate::init();

    fun::create_super_admin().await;
    let tls = tls::builder();

    let server = HttpServer::new(move || {
        App::new()
            .app_data(gcfg.clone())
            .wrap(middleware::NormalizePath::new(TrailingSlash::Trim))
            .wrap(middleware::Compress::default())
            .wrap(middleware::Logger::default())
            .wrap(cors::middleware::configure())
            .wrap(from_fn(ctx::middleware))
            .configure(cfg::configure)
            .configure(rts::configure)
            .configure(gtw::configure)
    });

    if tls_enabled == "true" {
        return server.bind_openssl(addrs, tls)?.run().await;
    }

    server.bind(addrs)?.run().await
}
