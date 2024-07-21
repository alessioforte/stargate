use actix_web::{get, web, HttpResponse, Responder};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct OAuthResponse {
    pub access_token: String,
    pub id_token: String,
}

#[derive(Deserialize)]
pub struct GoogleUserResult {
    pub id: String,
    pub email: String,
    pub verified_email: bool,
    pub name: String,
    pub given_name: String,
    pub family_name: String,
    pub picture: String,
    pub locale: String,
}

#[get("")]
async fn login() -> impl Responder {
    HttpResponse::Ok().body("Google login")
}

#[get("/callback")]
async fn callback() -> impl Responder {
    HttpResponse::Ok().body("Google callback")
}

pub fn routes() -> actix_web::Scope {
    web::scope("/google").service(login).service(callback)
}
