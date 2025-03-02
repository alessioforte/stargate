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
        crate::rts::health::get,
        crate::rts::account::profile,
        crate::rts::account::login,
        crate::rts::account::refresh,
        crate::rts::account::logout,
        crate::rts::account::forgot_password,
        crate::rts::account::change_password,
        crate::rts::signup::signup_request,
        crate::rts::signup::signup_confirm,
        crate::rts::signup::signup_complete,
    ),
    info(description = "Stargate APIs documentation")
)]
pub struct ApiDoc;

pub fn configure(cfg: &mut ServiceConfig) {
    let base_path = std::env::var("API_BASE_PATH").unwrap_or_else(|_| "".to_string());
    cfg.service(
        actix_web::web::scope(&base_path)
            .service(health::get)
            .service(docs::routes())
            .service(account::routes())
            .service(oauth::routes())
            .service(signup::routes())
            .service(admin::routes()),
    );
}
