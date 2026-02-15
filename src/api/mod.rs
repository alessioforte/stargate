mod account;
mod admin;
mod docs;
mod health;
mod mid;
mod oauth;
mod signup;

use actix_web::{guard, middleware::from_fn, web::ServiceConfig};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::health::get,
        crate::api::admin::configurations::get_configurations,
        crate::api::admin::configurations::update_configurations,
        crate::api::admin::users::get_users,
        crate::api::admin::users::create_user,
        crate::api::admin::users::get_user,
        crate::api::admin::users::update_user,
        crate::api::admin::users::patch_user,
        crate::api::admin::users::update_user_attrs,
        crate::api::admin::users::patch_user_attrs,
        crate::api::admin::users::delete_user,
        crate::api::admin::users::get_user_organizations,
        crate::api::admin::users::get_organization_users,
        crate::api::admin::users::add_user_to_organization,
        crate::api::admin::users::remove_user_from_organization,
        crate::api::admin::api_keys::get_api_keys,
        crate::api::admin::api_keys::create_api_key,
        crate::api::admin::api_keys::get_api_key,
        crate::api::admin::api_keys::delete_api_key,
        crate::api::admin::api_keys::revoke_api_key,
        crate::api::admin::api_keys::update_api_key_attrs,
        crate::api::admin::api_keys::patch_api_key_attrs,
        crate::api::admin::admin_keys::get_admin_keys,
        crate::api::admin::admin_keys::get_admin_key,
        crate::api::admin::admin_keys::create_admin_key,
        crate::api::admin::admin_keys::update_admin_key_permissions,
        crate::api::admin::admin_keys::revoke_admin_key,
        crate::api::admin::admin_keys::delete_admin_key,
        crate::api::admin::service_accounts::get_service_accounts,
        crate::api::admin::service_accounts::get_service_account,
        crate::api::admin::service_accounts::create_service_account,
        crate::api::admin::service_accounts::update_service_account,
        crate::api::admin::service_accounts::delete_service_account,
        crate::api::admin::organizations::get_organizations,
        crate::api::admin::organizations::get_organization,
        crate::api::admin::organizations::create_organization,
        crate::api::admin::organizations::update_organization,
        crate::api::admin::organizations::delete_organization,
        crate::api::account::profile::handler,
        crate::api::account::login::handler,
        crate::api::account::refresh_token::handler,
        crate::api::account::logout::handler,
        crate::api::account::credentials::forgot::handler,
        crate::api::account::credentials::reset::handler,
        crate::api::oauth::github::login,
        crate::api::oauth::google::login,
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

const API_PREFIXES: &[&str] = &[
    "/health", "/docs", "/account", "/oauth", "/signup", "/admin",
];

pub fn configure(cfg: &mut ServiceConfig) {
    let base_path = std::env::var("API_BASE_PATH").unwrap_or_else(|_| "".to_string());

    let mut scope = actix_web::web::scope(&base_path)
        .wrap(from_fn(mid::middleware))
        .service(health::get)
        .service(docs::routes())
        .service(account::routes())
        .service(oauth::routes())
        .service(signup::routes())
        .service(admin::routes());

    // When the base path is empty, the scope matches all requests, which causes
    // gateway requests to go through the API middleware. Add a guard so only
    // requests targeting known API paths enter this scope.
    if base_path.is_empty() {
        scope = scope.guard(guard::fn_guard(|ctx| {
            let path = ctx.head().uri.path();
            API_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
        }));
    }

    cfg.service(scope);
}
