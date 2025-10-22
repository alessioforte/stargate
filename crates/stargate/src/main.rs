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
    db::init().await;
    let s = store::init();
    let data_gate = gate::init(std::sync::Arc::new(s.clone()));
    let tls = tls::builder();

    fun::create_super_admin().await;

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::NormalizePath::new(TrailingSlash::Trim))
            .wrap(middleware::Logger::default())
            .wrap(cors::configure())
            .app_data(data_gate.clone())
            .configure(cfg::configure)
            .configure(rts::configure)
            .configure(gtw::configure)
    })
    .bind_openssl(format!("0.0.0.0:{}", port), tls)?
    .run()
    .await
}
