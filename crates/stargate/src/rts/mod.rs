pub mod account;
pub mod admin;
pub mod docs;
pub mod health;
pub mod oauth;
pub mod signup;
pub mod well_known;

use actix_web::web::ServiceConfig;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::rts::health::get,
        crate::rts::account::profile::handler,
        crate::rts::account::login::handler,
        crate::rts::account::refresh_token::handler,
        crate::rts::account::logout::handler,
        crate::rts::account::credentials::forgot::handler,
        crate::rts::account::credentials::reset::handler,
        crate::rts::signup::request::handler,
        crate::rts::signup::verification::handler,
        crate::rts::signup::complete::handler,
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
