use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::http::request::RequestExt;
use axum::Json;
use axum::extract::{FromRequest, Request};
use axum::response::Response;

use crate::act::otp::service::{complete_passwordless_email_otp, start_passwordless_email_otp};
use crate::act::otp::types::{
    OtpChallengeResponse, PasswordlessEmailOtpRequestBody, PasswordlessEmailOtpVerifyRequestBody,
};

#[utoipa::path(
    post,
    path = "/account/login/otp/email",
    tags = ["Account"],
    summary = "Request Passwordless Email OTP",
    description = "Start a passwordless login by sending an email one-time password. The response is generic even when the account does not exist.",
    request_body = PasswordlessEmailOtpRequestBody,
    responses(
        (status = 200, description = "OK", body = OtpChallengeResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 503, description = "Service Unavailable", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn post_login_email_otp(
    req: Request,
) -> Result<Json<OtpChallengeResponse>, ErrorResponse> {
    let client_ip = req.get_client_ip();
    let Json(body) = Json::<PasswordlessEmailOtpRequestBody>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;
    Ok(Json(
        start_passwordless_email_otp(&body.email, &client_ip).await?,
    ))
}

#[utoipa::path(
    put,
    path = "/account/login/otp/email",
    tags = ["Account"],
    summary = "Verify Passwordless Email OTP",
    description = "Verify a passwordless email OTP challenge and issue normal session tokens.",
    request_body = PasswordlessEmailOtpVerifyRequestBody,
    responses(
        (status = 200, description = "OK", body = crate::api::account::AuthResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn put_login_email_otp(req: Request) -> Result<Response, ErrorResponse> {
    let Json(body) = Json::<PasswordlessEmailOtpVerifyRequestBody>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;
    let (user, auth_time) = complete_passwordless_email_otp(&body.challenge_id, &body.code).await?;
    crate::api::account::session::issue_user_session(user, auth_time, None).await
}
