use crate::err::ErrorResponse;
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
        (status = 404, description = "No social sign-in provider is configured (`feature.disabled`)", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn post_state() -> Result<Json<StateResponse>, ErrorResponse> {
    let state =
        crate::iam::fed::state::create_oauth_state(&crate::iam::fed::state::OAuthState::default())
            .await?;
    Ok(Json(StateResponse { state }))
}
