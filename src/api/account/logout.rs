use crate::data::store::delete_confirmed;
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::auth::jwt::jwt_config;
use crate::iam::auth::revocation::{RevocationConfirmations, revoke_claims};
use crate::web::messages::{MessageCode, MessageResponse};
use crate::web::request::RequestExt;
use axum::Json;
use axum::extract::Request;

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
    let mut confirmations = RevocationConfirmations::default();
    confirmations.record(revoke_claims(&claims).await)?;

    match claims.sub_id.as_deref() {
        Some(user_id) => {
            confirmations.record(crate::iam::sess::registry::end_session(user_id, &sid).await)?;
        }
        None => {
            confirmations.record(delete_confirmed(&store::keys::session::subject(&sid)).await)?;
        }
    }

    confirmations.finish(())?;
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
    use crate::web::request::RequestExt;

    let session = crate::iam::sess::registry::authenticated_session(req.get_token()).await?;

    let revoked = crate::iam::sess::registry::revoke_all_sessions(&session.user.id).await?;
    tracing::info!(user_id = %session.user.id, revoked, "user logged out everywhere");

    Ok(Json(MessageResponse::new(
        MessageCode::AllSessionsLoggedOut,
    )))
}
