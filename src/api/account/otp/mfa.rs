use crate::act::otp::service::{complete_mfa_challenge, list_mfa_methods, start_mfa_challenge};
use crate::act::otp::types::{
    MfaChallengeRequestBody, MfaChallengeVerifyRequestBody, MfaMethodsResponse,
    MfaVerificationResponse, OtpChallengeResponse,
};
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::ext::RequestExt;
use axum::Json;
use axum::extract::{FromRequest, Path, Request};

#[utoipa::path(
    get,
    path = "/account/mfa/methods",
    tags = ["Account", "MFA"],
    summary = "List MFA Methods",
    description = "Return the effective MFA policy and available methods for the authenticated user.",
    responses(
        (status = 200, description = "OK", body = MfaMethodsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn get_mfa_methods(req: Request) -> Result<Json<MfaMethodsResponse>, ErrorResponse> {
    Ok(Json(list_mfa_methods(req.get_token()).await?))
}

#[utoipa::path(
    post,
    path = "/account/mfa/challenges",
    tags = ["Account", "MFA"],
    summary = "Request MFA Challenge",
    description = "Create an MFA challenge using one of the authenticated user's available methods. Email is the first supported method.",
    request_body = MfaChallengeRequestBody,
    responses(
        (status = 200, description = "OK", body = OtpChallengeResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 503, description = "Service Unavailable", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn post_mfa_challenge(req: Request) -> Result<Json<OtpChallengeResponse>, ErrorResponse> {
    let client_ip = req.get_client_ip();
    let token = req.get_token();
    let Json(body) = Json::<MfaChallengeRequestBody>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;

    Ok(Json(
        start_mfa_challenge(token, &client_ip, body.method, body.purpose.as_deref()).await?,
    ))
}

#[utoipa::path(
    put,
    path = "/account/mfa/challenges/{challenge_id}",
    tags = ["Account", "MFA"],
    summary = "Verify MFA Challenge",
    description = "Verify an MFA challenge and create a short-lived step-up verification marker for its purpose.",
    params(("challenge_id" = String, Path, description = "MFA challenge ID")),
    request_body = MfaChallengeVerifyRequestBody,
    responses(
        (status = 200, description = "OK", body = MfaVerificationResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Not Found", body = ErrorResponse),
        (status = 409, description = "Conflict", body = ErrorResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn put_mfa_challenge(
    Path(challenge_id): Path<String>,
    req: Request,
) -> Result<Json<MfaVerificationResponse>, ErrorResponse> {
    let token = req.get_token();
    let Json(body) = Json::<MfaChallengeVerifyRequestBody>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;

    Ok(Json(
        complete_mfa_challenge(token, &challenge_id, &body.code).await?,
    ))
}
