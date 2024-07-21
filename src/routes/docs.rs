use crate::errors::ErrorResponse;
use crate::routes::ApiDoc;
use actix_web::{get, HttpResponse, Scope};
use utoipa::OpenApi;

#[get("")]
pub async fn get_api_doc() -> Result<HttpResponse, ErrorResponse> {
    Ok(HttpResponse::Ok().json(ApiDoc::openapi()))
}

pub fn routes() -> Scope {
    Scope::new("/docs").service(get_api_doc)
}
