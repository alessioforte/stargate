use actix_web::{get, web, HttpResponse, Responder};

#[get("")]
async fn login() -> impl Responder {
    HttpResponse::Ok().body("Github login")
}

#[get("/callback")]
async fn callback() -> impl Responder {
    HttpResponse::Ok().body("Github callback")
}

pub fn routes() -> actix_web::Scope {
    web::scope("/github").service(login).service(callback)
}
