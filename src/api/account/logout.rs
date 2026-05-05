use crate::err::{ErrorResponse, HttpError};
use crate::etc::ext::RequestExt;
use crate::etc::jwt::jwt_config;
use crate::etc::msg::MessageResponse;
use crate::etc::store::use_store;
use axum::Json;
use axum::extract::Request;
use store::Store;

#[utoipa::path(
    delete,
    path = "/account/logout",
    tags = ["Account"],
    summary = "User Logout",
    description = "Log out the currently authenticated user by invalidating their session token.",
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn delete_logout(req: Request) -> Result<Json<MessageResponse>, ErrorResponse> {
    let token = req.get_token().ok_or_else(|| {
        ErrorResponse::from(HttpError::Unauthorized("Token not found".to_string()))
    })?;

    let claims = jwt_config()
        .validate_token(&token)
        .map_err(|_| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;

    let sid = claims.sid.clone().unwrap_or_default();
    crate::act::token_revocation::revoke_claims(&claims)
        .await
        .map_err(ErrorResponse::internal)?;

    use_store()
        .delete(&sid)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(
        "User logged out successfully",
        "logout_success",
    )))
}
