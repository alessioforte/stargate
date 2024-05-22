use std::env;

use actix_web::{get, HttpResponse, Responder};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct Health {
    name: &'static str,
    version: &'static str,
}

#[get("/health")]
pub async fn get() -> impl Responder {
    let version = env!("CARGO_PKG_VERSION");

    HttpResponse::Ok().json(Health {
        name: "Stargate is running!",
        version,
    })
}
