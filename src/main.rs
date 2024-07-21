mod cfg;
mod config;
mod errors;
mod models;
mod modules;
mod routes;
mod services;
mod utils;

use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{middleware, App, HttpServer};
use dotenvy::dotenv;
use log;
use pretty_env_logger;
use services::db;
use std::env;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("{}", cfg::LOGO);

    dotenv().ok();
    pretty_env_logger::init();
    db::init().await;

    // create super admin user
    utils::create_super_admin().await;

    // rate limiter middleware
    let governor_config = GovernorConfigBuilder::default()
        .per_second(2)
        .burst_size(5)
        .finish()
        .unwrap();

    let port = env::var("PORT").unwrap_or_else(|_| "5050".to_string());
    log::info!("Starting server on port {}", port);

    HttpServer::new(move || {
        App::new()
            .configure(cfg::app_data)
            .configure(routes::configure)
            .wrap(middleware::NormalizePath::new(
                middleware::TrailingSlash::Trim,
            ))
            .wrap(middleware::Logger::default())
            .wrap(Governor::new(&governor_config))
            .wrap(Cors::permissive())
    })
    .bind(format!("127.0.0.1:{}", port))?
    .run()
    .await
}
