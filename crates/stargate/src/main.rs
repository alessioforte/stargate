mod act;
mod err;
mod etc;
mod fun;
mod gtw;
mod rts;

use crate::etc::{cfg, cors, db, gate, jwt, logo, store, tls};
use actix_web::{App, HttpServer, middleware, middleware::TrailingSlash};
use dotenvy::dotenv;
use std::env;

// =^.^=
// 🦀
// Stargate ✨
// 🚀

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    pretty_env_logger::init();

    let port = env::var("PORT").unwrap_or_else(|_| "5050".to_string());
    let version = env!("CARGO_PKG_VERSION");

    println!("{}", logo::LOGO);
    log::info!("Version: {}", version);
    log::info!("Starting server on port {}", port);

    jwt::init();
    store::init();
    db::init().await;
    let gcfg = gate::init();

    fun::create_super_admin().await;
    let tls = tls::builder();

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::NormalizePath::new(TrailingSlash::Trim))
            .wrap(middleware::Logger::default())
            .wrap(cors::configure())
            .app_data(gcfg.clone())
            .configure(cfg::configure)
            .configure(rts::configure)
            .configure(gtw::configure)
    })
    .bind_openssl(format!("0.0.0.0:{}", port), tls)?
    .run()
    .await
}
