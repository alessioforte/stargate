mod config;
mod models;
mod modules;
mod routes;
mod services;

use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{web::Data, App, HttpServer};
use serde_yaml;
// use services::cache;
use services::db;

use config::{save_config_into_hashmap, Config, ConfigYAML};
use dotenv::dotenv;
use env_logger;
use log;
use routes::{account, admin, gateway, health, registrations};
use std::env;

// TODO: Handle errors

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();
    db::connect().await;
    // cache::connect().await;

    // load the configuration file
    let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| "".to_string());
    let config_file = env::var("CONFIG_FILE").unwrap_or_else(|_| "config.yaml".to_string());
    let config = std::fs::read_to_string(format!("{}/{}", config_path, config_file))
        .expect("Unable to read config file");
    let config: ConfigYAML = serde_yaml::from_str(&config).expect("Unable to parse config file");
    let config = Config {
        services: save_config_into_hashmap(&config),
    };
    let config_data = Data::new(config);

    // Add rate limiter middleware
    let governor_config = GovernorConfigBuilder::default()
        .per_second(2)
        .burst_size(5)
        .finish()
        .unwrap();

    let port = env::var("PORT").unwrap_or_else(|_| "5055".to_string());
    log::info!("Starting server on port {}", port);

    HttpServer::new(move || {
        App::new()
            .wrap(Governor::new(&governor_config))
            .app_data(config_data.clone())
            .service(health::get)
            .service(account::routes())
            .service(registrations::routes())
            .service(admin::routes())
            .service(gateway::handle_request)
            .wrap(Cors::permissive())
    })
    .bind(format!("127.0.0.1:{}", port))?
    .run()
    .await
}
