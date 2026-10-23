pub mod github;
pub mod google;
pub(crate) mod state;

pub use state::post_state;

/// Each route answers `404 feature.disabled` while its provider has no
/// credentials (`/oauth/state` while none has).
pub fn router() -> axum::Router {
    use crate::etc::features::{Feature, require_feature};
    use axum::middleware::{from_fn, from_fn_with_state};
    use axum::routing::{get, post};

    let state = axum::Router::new()
        .route("/oauth/state", post(post_state))
        .layer(from_fn(crate::etc::http::origin::trusted_origin_middleware))
        .layer(from_fn_with_state(Feature::Social, require_feature));

    axum::Router::new()
        .merge(state)
        .route(
            "/oauth/github",
            get(github::get_github)
                .layer(from_fn_with_state(Feature::SocialGithub, require_feature)),
        )
        .route(
            "/oauth/google",
            get(google::get_google)
                .layer(from_fn_with_state(Feature::SocialGoogle, require_feature)),
        )
}
