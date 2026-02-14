use super::SignupRequestBody;
use crate::{
    act,
    err::{ErrorResponse, HttpError},
    etc::{self, msg::MessageResponse},
};
use actix_web::{HttpResponse, post, web};
use smtp::{Smtp, Template};

#[utoipa::path(
    context_path = "/signup",
    path = "",
    tags = ["Signup"],
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 409, description = "Conflict", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),

    )
)]
#[post("")]
pub async fn handler(body: web::Json<SignupRequestBody>) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();

    let user = match crate::db::get_user_by_username(&body.email).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if user.is_some() {
        return Err(ErrorResponse::from(HttpError::Conflict(
            "User already exists".to_string(),
        )));
    }

    let sr = act::check_email_verification_request(&body.email)
        .await
        .map_err(|e| {
            ErrorResponse::from(HttpError::InternalServerError(format!(
                "Failed to check signup request: {}",
                e
            )))
        })?;
    if sr.is_some() {
        return Err(ErrorResponse::from(HttpError::Conflict(
            "Signup request already exists".to_string(),
        )));
    }

    let sid = act::create_email_verification_request(&body.email)
        .await
        .map_err(|e| {
            ErrorResponse::from(HttpError::InternalServerError(format!(
                "Failed to create signup request: {}",
                e
            )))
        })?;

    let claim = jwt::Claims::default()
        .subject("signup_request".to_string())
        .sid(sid.clone())
        .email(body.email.clone());

    let jwt = crate::etc::jwt::jwt_config();
    let token = match jwt.generate_token(&claim) {
        Ok(token) => token,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    let sender = Smtp::new()
        .template(Template::SignupRequest)
        .to(body.email.clone())
        .token(token)
        .build()
        .send();

    match sender {
        Ok(_) => {
            let message = etc::msg::MessageResponse::new(
                "A signup request has been sent to your email. Please check your inbox.",
                "signup_request",
            );

            Ok(HttpResponse::Ok().json(web::Json(message)))
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    }
}
