use crate::err::ErrorResponse;
use crate::fun::get_base_url;
use axum::Json;
use utoipa::{OpenApi, openapi::Server};

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
