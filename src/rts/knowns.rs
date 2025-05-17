use crate::err::ErrorResponse;
use actix_web::{get, HttpResponse};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
struct Key {
    kty: String,
    // use: String,
    kid: String,
    x5t: String,
    n: String,
    e: String,
}

#[derive(Debug, Serialize, ToSchema)]
struct Jwks {
    keys: Vec<Key>,
}

#[utoipa::path(
    path = "/knowns",
    responses(
        (status = 200, description = "OK", body = Jwks)
    )
)]
#[get("/knowns")]
pub async fn get() -> Result<HttpResponse, ErrorResponse> {
    Ok(HttpResponse::Ok().json(()))
}
