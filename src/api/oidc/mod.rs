mod authorization_codes;
pub(crate) mod authorize;
mod pkce;
mod refresh_tokens;
mod shared;
pub(crate) mod token;
pub(crate) mod token_ops;
pub(crate) mod userinfo;
pub(crate) mod well_known;

pub use authorize::get_authorize;
pub use token::post_token;
pub use token_ops::{post_introspect, post_revoke};
pub use userinfo::{get_userinfo, post_userinfo};
pub use well_known::{get_jwks, get_oauth_metadata, get_openid_configuration};

use axum::middleware::from_fn;

pub fn router() -> axum::Router {
    use axum::routing::{get, post};

    let protected = axum::Router::new()
        .route("/oauth/introspect", post(post_introspect))
        .route("/oauth/revoke", post(post_revoke))
        .layer(from_fn(crate::api::admin::extract_authorization));

    axum::Router::new()
        .route("/.well-known/jwks.json", get(get_jwks))
        .route(
            "/.well-known/oauth-authorization-server",
            get(get_oauth_metadata),
        )
        .route(
            "/.well-known/openid-configuration",
            get(get_openid_configuration),
        )
        .route("/oauth/authorize", get(get_authorize))
        .route("/oauth/token", post(post_token))
        .route("/oauth/userinfo", get(get_userinfo).post(post_userinfo))
        .merge(protected)
}
