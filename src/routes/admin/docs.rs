use crate::errors::ErrorResponse;
use crate::routes::admin::ApiDoc;
use actix_web::{get, HttpResponse, Scope};
use actix_web_grants::protect;
use utoipa::OpenApi;

#[get("")]
#[protect("SUPER_ADMIN")]
pub async fn get_api_doc() -> Result<HttpResponse, ErrorResponse> {
    Ok(HttpResponse::Ok().json(ApiDoc::openapi()))
}

pub fn routes() -> Scope {
    Scope::new("/docs").service(get_api_doc)
}
