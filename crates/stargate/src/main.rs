mod act;
mod err;
mod etc;
mod gtw;
mod mid;
mod rts;

use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{middleware, middleware::TrailingSlash, web::to, web::Data, App, HttpServer};
use dotenvy::dotenv;
use gate::Gate;
use std::env;

// =^.^=

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

    // create super admin user
    act::create_super_admin().await;

    // rate limiter middleware
    let governor_config = GovernorConfigBuilder::default()
        .seconds_per_request(2)
        .burst_size(32)
        .finish()
        .unwrap();

    let secret_key = actix_web::cookie::Key::generate();
    let tls = etc::tls::builder();

    let gate = Gate::init();
    gate.watch_file();
    let data = Data::new(gate);

    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .configure(etc::cfg::app_data)
            .configure(rts::configure)
            .default_service(to(gtw::handler))
            .wrap(middleware::NormalizePath::new(TrailingSlash::Trim))
            .wrap(mid::cookie_session(secret_key.clone()))
            .wrap(middleware::Logger::default())
            .wrap(Governor::new(&governor_config))
            .wrap(Cors::permissive())
    })
    .bind_openssl(format!("0.0.0.0:{}", port), tls)?
    .run()
    .await
}
