mod actions;
mod cfg;
mod data;
mod errors;
mod middlewares;
mod models;
mod modules;
mod routes;
mod services;

use crate::data::gate::Gate;
use crate::data::state::State;
use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::web::Data;
use actix_web::{middleware, middleware::TrailingSlash, App, HttpServer};
use dotenvy::dotenv;
use services::db;
use std::env;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    pretty_env_logger::init();

    println!("{}", LOGO);
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

    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .app_data(gate.clone())
            .configure(cfg::app_data)
            .configure(routes::configure)
            .wrap(middleware::NormalizePath::new(TrailingSlash::Trim))
            .wrap(middlewares::cookie_session(secret_key.clone()))
            .wrap(middleware::Logger::default())
            .wrap(Governor::new(&governor_config))
            .wrap(Cors::permissive())
    })
    .bind(format!("0.0.0.0:{}", port))?
    .run()
    .await
}

pub const LOGO: &str = "
    .d88888b.   dP                                         dP
    88.         88                                         88
    'Y88888b. d8888P .d8888b. 88d888b. .d8888b. .d8888b. d8888P .d8888b.
          '8b   88   88'  '88 88'  '88 88'  '88 88'  '88   88   88ooood8
    d8'   .8P   88   88.  .88 88       88.  .88 88.  .88   88   88.  ...
     Y88888P    dP   '88888P8 dP       '8888P88 '88888P8   dP   '88888P'
                                            .88
                                        d8888P
";
