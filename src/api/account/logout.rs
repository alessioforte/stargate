use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::auth::jwt::jwt_config;
use crate::etc::http::messages::{MessageCode, MessageResponse};
use crate::etc::http::request::RequestExt;
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
    let token = req
        .get_token()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenMissing))?;

    let claims = jwt_config()
        .validate_session_access_token(&token)
        .map_err(|_| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;

    let sid = claims.sid.clone().unwrap_or_default();
    crate::act::token_revocation::revoke_claims(&claims)
        .await
        .map_err(ErrorResponse::internal)?;

    use_store()
        .delete(&sid)
        .await
        .map_err(ErrorResponse::internal)?;

    if let Some(user_id) = claims.sub_id.as_deref() {
        crate::act::sessions::forget_session(user_id, &sid).await;
    }

    Ok(Json(MessageResponse::new(MessageCode::LogoutCompleted)))
}

#[utoipa::path(
    post,
    path = "/account/logout/all",
    tags = ["Account"],
    summary = "Logout Everywhere",
    description = "Invalidate every session of the authenticated user, including forked org-context sessions on other devices. Access and refresh tokens stop working immediately.",
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn post_logout_all(req: Request) -> Result<Json<MessageResponse>, ErrorResponse> {
    use crate::etc::http::request::RequestExt;

    let session = crate::act::sessions::authenticated_session(req.get_token()).await?;

    let revoked = crate::act::sessions::revoke_all_sessions(&session.user.id)
        .await
        .map_err(ErrorResponse::internal)?;
    tracing::info!(user_id = %session.user.id, revoked, "user logged out everywhere");

    Ok(Json(MessageResponse::new(
        MessageCode::AllSessionsLoggedOut,
    )))
}
