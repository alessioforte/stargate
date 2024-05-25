mod config;
mod models;
mod modules;
mod routes;
mod services;
mod utils;

use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{middleware, web::Data, App, HttpServer};
use config::get_config_from_yaml;
use dotenv::dotenv;
use env_logger;
use log;
use routes::{account, admin, gateway, health, signup};
use services::db;

use std::env;

// TODO: Handle errors

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();
    db::connect().await;

    // create super admin user
    utils::create_super_admin().await;

    // get config from yaml
    let config = get_config_from_yaml();
    let config_data = Data::new(config);

    // rate limiter middleware
    let governor_config = GovernorConfigBuilder::default()
        .per_second(2)
        .burst_size(5)
        .finish()
        .unwrap();

    let port = env::var("PORT").unwrap_or_else(|_| "5055".to_string());
    log::info!("Starting server on port {}", port);

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::NormalizePath::new(
                middleware::TrailingSlash::Trim,
            ))
            .wrap(middleware::Logger::default())
            .wrap(Governor::new(&governor_config))
            .app_data(config_data.clone())
            .service(health::get)
            .service(account::routes())
            .service(signup::routes())
            .service(admin::routes())
            .service(gateway::handle_request)
            .wrap(Cors::permissive())
    })
    .bind(format!("127.0.0.1:{}", port))?
    .run()
    .await
}
