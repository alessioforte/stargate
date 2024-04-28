mod config;
mod controllers;
mod db;
mod models;
mod services;
mod utils;

use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{web::Data, App, HttpServer};
use serde_yaml;

use config::{save_config_into_hashmap, Config, ConfigYAML};
use dotenv::dotenv;
use env_logger;
use log;
use std::env;

// TODO: Handle errors

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();
    db::init().await;

    // load the configuration file
    let config_file = env::var("CONFIG_FILE").unwrap_or_else(|_| "config.yaml".to_string());
    let config = std::fs::read_to_string(config_file).expect("Unable to read config file");
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
            .service(controllers::health::get)
            .service(controllers::account::routes())
            .service(controllers::registrations::routes())
            .service(controllers::admin::routes())
            .service(controllers::gateway::handle_request)
            .wrap(Cors::permissive())
    })
    .bind(format!("127.0.0.1:{}", port))?
    .run()
    .await
}
