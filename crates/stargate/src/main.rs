mod act;
mod err;
mod etc;
mod gtw;
mod rts;

use actix_web::{App, HttpServer, middleware, middleware::TrailingSlash, web::to};
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

    println!("{}", etc::logo::LOGO);
    log::info!("Version: {}", version);
    log::info!("Starting server on port {}", port);

    etc::jwt::init();
    etc::db::init().await;
    let store = etc::store::init();
    let tls = etc::tls::builder();
    let data_gate = etc::gate::init();
    let limiter = etc::lim::init(store.clone());

    // create super admin user
    act::create_super_admin().await;

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::NormalizePath::new(TrailingSlash::Trim))
            .wrap(middleware::Logger::default())
            .wrap(etc::cors::configure())
            .app_data(data_gate.clone())
            .app_data(limiter.clone())
            .configure(etc::cfg::app_data)
            .configure(rts::configure)
            .default_service(to(gtw::handler))
    })
    .bind_openssl(format!("0.0.0.0:{}", port), tls)?
    .run()
    .await
}
