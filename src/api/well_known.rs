use crate::err::ErrorResponse;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct Key {
    kty: String,
    kid: String,
    x5t: String,
    n: String,
    e: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Jwks {
    keys: Vec<Key>,
}

#[utoipa::path(
    get,
    path = "/.well-known/jwks.json",
    responses(
        (status = 200, description = "OK", body = Jwks)
    )
)]
pub async fn get_jwks() -> Result<Json<serde_json::Value>, ErrorResponse> {
    Ok(Json(serde_json::Value::Null))
}
