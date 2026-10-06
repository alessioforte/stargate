pub mod github;
pub mod google;
pub(crate) mod state;

pub use state::post_state;

pub fn router() -> axum::Router {
    use axum::middleware::from_fn;
    use axum::routing::{get, post};

    let state = axum::Router::new()
        .route("/oauth/state", post(post_state))
        .layer(from_fn(crate::etc::http::origin::trusted_origin_middleware));

    axum::Router::new()
        .merge(state)
        .route("/oauth/github", get(github::get_github))
        .route("/oauth/google", get(google::get_google))
}
