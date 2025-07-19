use super::SignupRequestBody;
use crate::{
    err::{ErrorResponse, HttpError},
    etc,
};
use actix_web::{post, web, HttpResponse};
use db::ent::{Action, ActionType};
use db::Transaction;
use smtp::{Smtp, Template};
use uuid::Uuid;

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
            )))
        }
    };

    if user.is_some() {
        // If a user already exists with this email, we return a conflict error
        return Err(ErrorResponse::from(HttpError::Conflict(
            "User already exists".to_string(),
        )));
    }
    // Check if a signup request already exists for this email
    let sr = match service
        .get_action_by_sub_and_type(&body.email, ActionType::EmailVerification)
        .await
    {
        Ok(request) => request,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    };

    if sr.is_some() && sr.unwrap().exp > chrono::Utc::now().timestamp() {
        // If a signup request already exists and is not expired, we return a conflict error
        return Err(ErrorResponse::from(HttpError::Conflict(
            "Signup request already exists".to_string(),
        )));
    }

    let uuid = Uuid::new_v4().to_string();

    let claim = jwt::Claims::default()
        .sub_id(uuid.clone())
        .email(body.email.clone());

    let jwt = crate::etc::jwt::jwt_config();
    let token = match jwt.generate_token(&claim) {
        Ok(token) => token,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    };

    let action = Action::new(
        body.email.clone(),
        ActionType::EmailVerification,
        60 * 60, // 1 hour expiration
        uuid.clone(),
    );

    // Send the signup request email
    let sender = Smtp::new()
        .template(Template::SignupRequest)
        .to(body.email.clone())
        .token(token)
        .build()
        .send();

    match sender {
        Ok(_) => {
            // If the email was sent successfully, create the signup request in the database
            match service.create_action(action).await {
                Ok(_) => {}
                Err(e) => {
                    return Err(ErrorResponse::from(HttpError::InternalServerError(
                        e.to_string(),
                    )))
                }
            }
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    }

    let message = etc::msg::MessageResponse::new(
        "A signup request has been sent to your email. Please check your inbox.".to_string(),
        "signup_request".to_string(),
    );

    Ok(HttpResponse::Ok().json(web::Json(message)))
}
