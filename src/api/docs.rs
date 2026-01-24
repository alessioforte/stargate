use super::ApiDoc;
use crate::err::ErrorResponse;
use crate::fun::get_base_url;
use actix_web::{HttpResponse, Scope, get};
use utoipa::{OpenApi, openapi::Server};

#[utoipa::path(
    context_path = "/docs",
    path = "",
    tags = ["Documentation"],
    responses(
        (status = 200, description = "OK")
    )
)]
#[get("")]
pub async fn get_api_doc() -> Result<HttpResponse, ErrorResponse> {
    let mut doc = ApiDoc::openapi();

    let base_url = get_base_url();
    doc.servers = Some(vec![Server::new(base_url)]);

    Ok(HttpResponse::Ok().json(doc))
}

pub fn routes() -> Scope {
    Scope::new("/docs").service(get_api_doc)
}
