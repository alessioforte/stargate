use actix_web::web;
// pub mod github;
pub mod google;

pub fn routes() -> actix_web::Scope {
    web::scope("/oauth").service(google::routes())
    // .service(github::routes())
}
