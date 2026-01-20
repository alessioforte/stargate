mod account;
mod admin;
mod docs;
mod health;
mod mid;
mod oauth;
mod signup;

use actix_web::{middleware::from_fn, web::ServiceConfig};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::health::get,
        crate::api::admin::configurations::get_configurations,
        crate::api::admin::configurations::update_configurations,
        crate::api::admin::users::get_users,
        crate::api::admin::users::create_user,
        crate::api::admin::users::delete_user,
        crate::api::admin::users::update_user,
        crate::api::admin::users::patch_user,
        crate::api::admin::api_keys::get_api_keys,
        crate::api::admin::api_keys::create_api_key,
        crate::api::admin::api_keys::delete_api_key,
        crate::api::account::profile::handler,
        crate::api::account::login::handler,
        crate::api::account::refresh_token::handler,
        crate::api::account::logout::handler,
        crate::api::account::credentials::forgot::handler,
        crate::api::account::credentials::reset::handler,
        crate::api::signup::request::handler,
        crate::api::signup::verification::handler,
        crate::api::signup::complete::handler,
        crate::api::docs::get_api_doc,
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
