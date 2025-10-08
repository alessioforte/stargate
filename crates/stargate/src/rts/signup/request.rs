use super::SignupRequestBody;
use crate::{
    act,
    err::{ErrorResponse, HttpError},
    etc,
};
use actix_web::{HttpResponse, post, web};
use db::Transaction;
use smtp::{Smtp, Template};

#[utoipa::path(
    context_path = "/signup",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[post("")]
pub async fn handler(body: web::Json<SignupRequestBody>) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();

    let service = crate::etc::db::service();
    let user = match service.get_user_by_username(&body.email).await {
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

    let uuid = act::create_email_verification_request(&body.email)
        .await
        .map_err(|e| {
            ErrorResponse::from(HttpError::InternalServerError(format!(
                "Failed to create signup request: {}",
                e
            )))
        })?;

    let claim = jwt::Claims::default()
        .sub_id(uuid.clone())
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
                "A signup request has been sent to your email. Please check your inbox."
                    .to_string(),
                "signup_request".to_string(),
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
