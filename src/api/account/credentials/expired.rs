use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use db::ent::{Credential, CredentialType, User};
use serde::Serialize;
use utoipa::ToSchema;

use crate::act;
use crate::err::ErrorResponse;
use crate::etc;

/// Returned by password login instead of session tokens when the password is
/// correct (and MFA, if required, verified) but older than the effective
/// policy's `max_age_days`. The token feeds the normal change-password flow.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordExpiredResponse {
    /// Always `true`.
    pub password_expired: bool,
    pub message: String,
    /// Change-password token accepted by `PUT /account/credentials`.
    pub reset_token: String,
    /// Reset token validity in seconds (upper bound).
    pub expires_in_secs: u64,
}

/// Returns the `202` password-expired response when the user's password is
/// past its max age, `None` otherwise. Must be called only after the caller
/// has fully authenticated the user (password verified, MFA completed when
/// required), because the response hands out a password-reset token.
pub(crate) async fn maybe_password_expired_response(
    user: &User,
    credential: Option<&Credential>,
) -> Result<Option<Response>, ErrorResponse> {
    let changed_at = match credential {
        Some(credential) => credential.updated_at,
        None => {
            let Some(credential) =
                crate::db::get_credential(&user.id, CredentialType::Password)
                    .await
                    .map_err(ErrorResponse::internal)?
            else {
                return Ok(None);
            };
            credential.updated_at
        }
    };

    if !act::password_policy::password_expired(&user.id, changed_at).await {
        return Ok(None);
    }

    // Reuse a pending change-password request when one exists (e.g. from the
    // forgot flow) so the emailed link stays valid; otherwise create one.
    let sid = match act::get_change_password_request(&user.email).await {
        Ok(Some(sid)) => sid,
        _ => act::create_change_password_request(&user.email)
            .await
            .map_err(ErrorResponse::internal)?,
    };

    let claim = jwt::Claims::default()
        .sub_id(user.id.clone())
        .email(user.email.clone())
        .sid(sid);
    let reset_token = etc::jwt::jwt_config()
        .generate_token(&claim)
        .map_err(ErrorResponse::internal)?;

    let body = PasswordExpiredResponse {
        password_expired: true,
        message: "Password has expired and must be changed".to_string(),
        reset_token,
        expires_in_secs: act::CHANGE_PASSWORD_REQUEST_TTL_SECS,
    };
    let mut response = Json(body).into_response();
    *response.status_mut() = StatusCode::ACCEPTED;
    Ok(Some(response))
}
