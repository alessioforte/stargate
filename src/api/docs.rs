use crate::err::{ErrorCode, ErrorResponse, ErrorType};
use crate::fun::get_base_url;
use axum::Json;
use serde::Serialize;
use utoipa::{OpenApi, openapi::Server};

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorCatalogEntry {
    #[schema(value_type = String, example = "user.not_found")]
    pub code: ErrorCode,
    pub status: u16,
    #[serde(rename = "type")]
    pub error_type: ErrorType,
    pub message: &'static str,
    pub link: String,
}

#[utoipa::path(
    get,
    path = "/docs",
    tags = ["Documentation"],
    responses(
        (status = 200, description = "OK")
    )
)]
pub async fn get_api_doc() -> Result<Json<utoipa::openapi::OpenApi>, ErrorResponse> {
    let mut doc = super::ApiDoc::openapi();
    doc.servers = Some(vec![Server::new(get_base_url())]);
    Ok(Json(doc))
}

#[utoipa::path(
    get,
    path = "/docs/errors",
    tags = ["Documentation"],
    responses(
        (status = 200, description = "Published API error catalog", body = [ErrorCatalogEntry])
    )
)]
pub async fn get_error_catalog() -> Json<Vec<ErrorCatalogEntry>> {
    Json(
        ErrorCode::ALL
            .iter()
            .map(|code| ErrorCatalogEntry {
                code: *code,
                status: code.status().as_u16(),
                error_type: code.error_type(),
                message: code.message(),
                link: code.url(),
            })
            .collect(),
    )
}
