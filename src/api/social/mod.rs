pub mod github;
pub mod google;
pub(crate) mod state;

pub use state::post_state;

pub fn router() -> axum::Router {
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/oauth/state", post(post_state))
        .route("/oauth/github", get(github::get_github))
        .route("/oauth/google", get(google::get_google))
}
