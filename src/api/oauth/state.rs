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
