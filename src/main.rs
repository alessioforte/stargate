mod actions;
mod errors;
mod etc;
mod gateway;
mod middlewares;
mod models;
mod modules;
mod routes;
mod services;

use crate::etc::gate::Gate;
use crate::etc::state::State;
use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{middleware, middleware::TrailingSlash, web::to, web::Data, App, HttpServer};
use dotenvy::dotenv;
use services::db;
use std::env;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    pretty_env_logger::init();

    println!("{}", etc::logo::LOGO);
    let port = env::var("PORT").unwrap_or_else(|_| "5050".to_string());
    let version = env!("CARGO_PKG_VERSION");
    log::info!("Starting server on port {}", port);
    log::info!("Version: {}", version);

    db::init().await;
    // create super admin user
    actions::create_super_admin().await;

    // rate limiter middleware
    let governor_config = GovernorConfigBuilder::default()
        .per_second(2)
        .burst_size(5)
        .finish()
        .unwrap();

    let data = Data::new(State::init());
    let gate = Gate::init();
    let secret_key = actix_web::cookie::Key::generate();
    gate.watch_file();
    let gate = Data::new(gate);

    let tls_config = etc::tls::config();

    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .app_data(gate.clone())
            .configure(etc::cfg::app_data)
            .configure(routes::configure)
            .default_service(to(gateway::handler))
            .wrap(middleware::NormalizePath::new(TrailingSlash::Trim))
            .wrap(middlewares::cookie_session(secret_key.clone()))
            .wrap(middleware::Logger::default())
            .wrap(Governor::new(&governor_config))
            .wrap(Cors::permissive())
    })
    .bind_rustls_0_23(format!("0.0.0.0:{}", port), tls_config)?
    .run()
    .await
}
