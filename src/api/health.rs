use crate::err::ErrorResponse;
use actix_web::{HttpResponse, get};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
struct Health {
    name: &'static str,
    version: &'static str,
    status: &'static str,
}

#[utoipa::path(
    path = "/health",
    tags = ["Health"],
    responses(
        (status = 200, description = "OK", body = Health),
        (status = 503, description = "Service Unavailable", body = Health)
    )
)]
#[get("/health")]
pub async fn get() -> Result<HttpResponse, ErrorResponse> {
    let version = env!("CARGO_PKG_VERSION");

    let db_ok = crate::db::ping().await.is_ok();

    if db_ok {
        Ok(HttpResponse::Ok().json(Health {
            name: "stargate",
            version,
            status: "healthy",
        }))
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(Health {
            name: "stargate",
            version,
            status: "unhealthy",
        }))
    }
}
