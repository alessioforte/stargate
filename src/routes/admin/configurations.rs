use crate::config::Config;
use actix_web::{get, web, web::Data, HttpResponse, Responder};
use actix_web_grants::protect;

#[get("")]
#[protect("SUPER_ADMIN")]
pub async fn get_configurations(config: Data<Config>) -> impl Responder {
    HttpResponse::Ok().json(web::Json(config.export()))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/configurations").service(get_configurations)
}
