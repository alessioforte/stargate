use super::{EmailVerificationResponse, SignupVerificationParams};
use crate::act;
use crate::err::{ErrorResponse, HttpError};
use axum::Json;
use axum::extract::Query;

#[utoipa::path(
    get,
    path = "/signup",
    tags = ["Signup"],
    params(("token" = String, Query, description = "Signup verification token")),
    responses(
        (status = 200, description = "OK", body = EmailVerificationResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 404, description = "Not Found", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn get_signup(
    Query(query): Query<SignupVerificationParams>,
) -> Result<Json<EmailVerificationResponse>, ErrorResponse> {
    let jwt_cfg = crate::etc::jwt::jwt_config();
    let claim = jwt_cfg
        .validate_token(&query.token)
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.to_string())))?;

    if claim.sub != "signup_request" {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Invalid token".to_string(),
        )));
    }

    let sid = claim
        .sid
        .clone()
        .ok_or_else(|| ErrorResponse::from(HttpError::BadRequest("Invalid token".to_string())))?;

    let request = act::get_email_verification_request(&sid)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::NotFound("Signup request not found".to_string()))
        })?;

    if let Some(ref claim_email) = claim.email
        && &request.email != claim_email
    {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Email does not match".to_string(),
        )));
    }

    if act::get_signup_request(&sid)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        act::create_signup_request(&sid, &request)
            .await
            .map_err(ErrorResponse::internal)?;
    }

    let new_claim = jwt::Claims::default()
        .subject("signup".to_string())
        .sub_id(sid)
        .email(request.email.clone());

    let token = jwt_cfg
        .generate_token(&new_claim)
        .map_err(ErrorResponse::internal)?;

    Ok(Json(EmailVerificationResponse {
        token,
        email: request.email,
        given_name: request.given_name,
        family_name: request.family_name,
        nickname: request.nickname,
        phone_number: request.phone_number,
    }))
}
