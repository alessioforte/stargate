use crate::errors::ErrorResponse;
use actix_web::{get, HttpResponse};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct Health {
    name: &'static str,
    version: &'static str,
}

#[utoipa::path(
    path = "/health",
    responses(
        (status = 200, description = "OK", body = Health)
    )
)]
#[get("/health")]
pub async fn get() -> Result<HttpResponse, ErrorResponse> {
    let version = env!("CARGO_PKG_VERSION");

    Ok(HttpResponse::Ok().json(Health {
        name: "Stargate is running!",
        version,
    }))
}
