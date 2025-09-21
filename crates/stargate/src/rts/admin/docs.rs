use super::ApiDoc;
use crate::err::ErrorResponse;
use actix_web::{HttpResponse, Scope, get};
use actix_web_grants::protect;
use utoipa::OpenApi;

#[utoipa::path(
    context_path = "/docs",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[get("")]
#[protect("SUPER_ADMIN")]
pub async fn get_api_doc() -> Result<HttpResponse, ErrorResponse> {
    Ok(HttpResponse::Ok().json(ApiDoc::openapi()))
}

pub fn routes() -> Scope {
    Scope::new("/docs").service(get_api_doc)
}
