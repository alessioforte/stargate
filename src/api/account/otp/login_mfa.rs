use crate::act::otp::service::{complete_login_mfa, maybe_start_login_mfa as start_login_mfa};
use crate::act::otp::types::MfaChallengeVerifyRequestBody;
use crate::err::{ErrorResponse, HttpError};
use axum::Json;
use axum::extract::{FromRequest, Path, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[utoipa::path(
    put,
    path = "/account/login/mfa/challenges/{challenge_id}",
    tags = ["Account", "MFA"],
    summary = "Verify Login MFA Challenge",
    description = "Verify a pending MFA challenge created during password login and issue normal session tokens.",
    params(("challenge_id" = String, Path, description = "Pending login MFA challenge ID")),
    request_body = MfaChallengeVerifyRequestBody,
    responses(
        (status = 200, description = "OK", body = crate::api::account::AuthResponse),
        (status = 202, description = "Password Expired", body = crate::api::account::credentials::expired::PasswordExpiredResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 409, description = "Conflict", body = ErrorResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn put_login_mfa_challenge(
    Path(challenge_id): Path<String>,
    req: Request,
) -> Result<Response, ErrorResponse> {
    let Json(body) = Json::<MfaChallengeVerifyRequestBody>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;

    let (user, auth_time, requested_org_id) = complete_login_mfa(&challenge_id, &body.code).await?;

    // The user is now fully authenticated (password + MFA); an expired
    // password swaps the session for a change-password token.
    if let Some(response) =
        crate::api::account::credentials::expired::maybe_password_expired_response(&user, None)
            .await?
    {
        return Ok(response);
    }

    crate::api::account::session::issue_user_session(user, auth_time, requested_org_id.as_deref())
        .await
}

pub(crate) async fn maybe_start_login_mfa(
    user: &db::ent::User,
    client_ip: &str,
    auth_time: usize,
    requested_org_id: Option<&str>,
) -> Result<Option<Response>, ErrorResponse> {
    let Some(body) = start_login_mfa(user, client_ip, auth_time, requested_org_id).await? else {
        return Ok(None);
    };

    let mut response = Json(body).into_response();
    *response.status_mut() = StatusCode::ACCEPTED;
    Ok(Some(response))
}
