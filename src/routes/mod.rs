pub mod account;
pub mod admin;
pub mod docs;
pub mod health;
pub mod oauth;
pub mod signup;

use actix_web::web::ServiceConfig;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::routes::health::get,
        crate::routes::account::profile,
        crate::routes::account::login,
        crate::routes::account::refresh,
        crate::routes::account::logout,
        crate::routes::account::forgot_password,
        crate::routes::account::change_password,
        crate::routes::signup::signup_request,
        crate::routes::signup::signup_confirm,
        crate::routes::signup::signup_complete,
    ),
    info(description = "Stargate documentation")
)]
pub struct ApiDoc;

pub fn configure(cfg: &mut ServiceConfig) {
    cfg.service(health::get)
        .service(docs::routes())
        .service(account::routes())
        .service(oauth::routes())
        .service(signup::routes())
        .service(admin::routes());
}
