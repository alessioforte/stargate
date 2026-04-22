pub mod github;
pub mod google;

use crate::err::{ErrorResponse, HttpError};
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct StateResponse {
    pub state: String,
}

#[utoipa::path(
    post,
    path = "/oauth/state",
    tags = ["OAuth"],
    responses(
        (status = 200, description = "OK"),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn post_state() -> Result<Json<StateResponse>, ErrorResponse> {
    let state = crate::act::oauth_state::create_oauth_state()
        .await
        .map_err(|e| {
            tracing::error!("Failed to create OAuth state: {}", e);
            ErrorResponse::from(HttpError::InternalServerError(
                "failed to generate oauth state".to_string(),
            ))
        })?;
    Ok(Json(StateResponse { state }))
}

pub(super) fn build_jwt_cookie(access_token: &str, max_age_secs: i64) -> String {
    let secure = crate::etc::tls::enabled().unwrap_or(false);
    let mut parts = vec![
        format!("jwt={}", access_token),
        "Path=/".to_string(),
        "HttpOnly".to_string(),
        "SameSite=Strict".to_string(),
        format!("Max-Age={}", max_age_secs),
    ];
    if secure {
        parts.push("Secure".to_string());
    }
    parts.join("; ")
}

pub fn router() -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/oauth/state", post(post_state))
        .route("/oauth/github", get(github::get_github))
        .route("/oauth/google", get(google::get_google))
}
