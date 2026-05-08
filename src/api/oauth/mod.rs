pub mod github;
pub mod google;

mod authorization_codes;
pub(crate) mod authorize;
mod pkce;
mod refresh_tokens;
mod shared;
pub(crate) mod state;
pub(crate) mod token;
pub(crate) mod token_ops;
pub(crate) mod userinfo;

pub use authorize::get_authorize;
pub use state::post_state;
pub use token::post_token;
pub use token_ops::{post_introspect, post_revoke};
pub use userinfo::{get_userinfo, post_userinfo};

use axum::middleware::from_fn;

pub fn router() -> axum::Router {
    use axum::routing::{get, post};

    let protected = axum::Router::new()
        .route("/oauth/introspect", post(post_introspect))
        .route("/oauth/revoke", post(post_revoke))
        .layer(from_fn(crate::api::admin::extract_grants));

    axum::Router::new()
        .route("/oauth/state", post(post_state))
        .route("/oauth/authorize", get(get_authorize))
        .route("/oauth/token", post(post_token))
        .route("/oauth/userinfo", get(get_userinfo).post(post_userinfo))
        .route("/oauth/github", get(github::get_github))
        .route("/oauth/google", get(google::get_google))
        .merge(protected)
}
