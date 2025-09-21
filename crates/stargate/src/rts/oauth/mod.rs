use actix_web::web;
pub mod github;
pub mod google;

// TODO: add routes in docs
// TODO: add more oauth providers

pub fn routes() -> actix_web::Scope {
    web::scope("/oauth")
        .service(google::routes())
        .service(github::routes())
}
