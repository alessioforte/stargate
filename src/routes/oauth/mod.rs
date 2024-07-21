use actix_web::web;
pub mod github;
pub mod google;

pub fn routes() -> actix_web::Scope {
    web::scope("/auth")
        .service(google::routes())
        .service(github::routes())
}
