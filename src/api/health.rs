use crate::err::ErrorResponse;
use axum::Json;
use axum::response::IntoResponse;
use http::StatusCode;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    name: &'static str,
    version: &'static str,
    status: &'static str,
}

#[utoipa::path(
    get,
    path = "/health",
    tags = ["Health"],
    responses(
        (status = 200, description = "OK", body = Health),
        (status = 503, description = "Service Unavailable", body = Health)
    )
)]
pub async fn get_health() -> Result<axum::response::Response, ErrorResponse> {
    let version = env!("CARGO_PKG_VERSION");

    let db_ok = crate::db::ping().await.is_ok();

    if db_ok {
        Ok((
            StatusCode::OK,
            Json(Health {
                name: "stargate",
                version,
                status: "healthy",
            }),
        )
            .into_response())
    } else {
        Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Health {
                name: "stargate",
                version,
                status: "unhealthy",
            }),
        )
            .into_response())
    }
}
