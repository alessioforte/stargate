mod account;
mod admin;
mod docs;
mod health;
mod mid;
mod oauth;
mod signup;
// pub mod well_known;

use actix_web::{middleware::from_fn, web::ServiceConfig};
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
        crate::rts::docs::get_api_doc,
    ),
    info(
        title = "Stargate APIs ✨",
        description = "APIs for user authentication and management in Stargate.",
    )
)]
pub struct ApiDoc;

pub fn configure(cfg: &mut ServiceConfig) {
    let mut base_path = std::env::var("API_BASE_PATH").unwrap_or_else(|_| "".to_string());
    if base_path.is_empty() || base_path == "/" {
        base_path = "/stargate".to_string();
    }

    cfg.service(
        actix_web::web::scope(&base_path)
            .wrap(from_fn(mid::middleware))
            .service(health::get)
            .service(docs::routes())
            .service(account::routes())
            .service(oauth::routes())
            .service(signup::routes())
            .service(admin::routes()),
    );
}
