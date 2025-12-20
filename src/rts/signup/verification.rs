use super::{EmailVerificationResponse, SignupVerificationParams};
use crate::act;
use crate::err::{ErrorResponse, HttpError};
use actix_web::{HttpResponse, get, web};

#[utoipa::path(
    context_path = "/signup",
    path = "",
    tags = ["Signup"],
    responses(
        (status = 200, description = "OK")
    )
)]
#[get("")]
pub async fn handler(
    query: web::Query<SignupVerificationParams>,
) -> Result<HttpResponse, ErrorResponse> {
    let query = query.into_inner();
    let token = query.token.clone();
    let jwt = crate::etc::jwt::jwt_config();
    let claim = match jwt.validate_token(&token) {
        Ok(claim) => claim,
        Err(e) => return Err(ErrorResponse::from(HttpError::BadRequest(e.to_string()))),
    };

    let sid = match claim.sid {
        Some(sid) => sid,
        None => {
            return Err(ErrorResponse::from(HttpError::BadRequest(
                "Invalid token".to_string(),
            )));
        }
    };

    let request = match act::get_email_verification_request(&sid).await {
        Ok(request) => request,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if request.is_none() {
        return Err(ErrorResponse::from(HttpError::NotFound(
            "Signup request not found".to_string(),
        )));
    }
    let request = request.unwrap();

    if claim.email.is_some() && request != claim.email.clone().unwrap() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Email does not match".to_string(),
        )));
    }

    let sid = match act::create_signup_request(&request).await {
        Ok(new_sid) => {
            let _ = act::delete_email_verification_request(&sid).await;
            new_sid
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    let email = claim.email.unwrap_or_default();
    let claim = jwt::Claims::default()
        .subject("signup".to_string())
        .sub_id(sid)
        .email(email.clone());

    let jwt = crate::etc::jwt::jwt_config();
    let token = match jwt.generate_token(&claim) {
        Ok(token) => token,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(web::Json(EmailVerificationResponse { token, email })))
}
