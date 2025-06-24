use crate::act::format_name;
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use actix_web::{post, web, HttpResponse};
use db::ent::{Action, ActionType};
use db::Transaction;
use serde::{Deserialize, Serialize};
use smtp::{Smtp, Template};
use utoipa::ToSchema;
use uuid::Uuid;
#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ForgotPasswordRequestBody {
    email: String,
}

#[utoipa::path(
    context_path = "/account",
    path = "/credentials",
    responses(
        (status = 200, description = "OK")
    )
)]
#[post("")]
pub async fn handler(
    body: web::Json<ForgotPasswordRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();

    let service = etc::db::service();
    let user = match service.get_user_by_username(&body.email).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    };

    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();
    let uuid = Uuid::new_v4().to_string();
    let claim = jwt::Claims::default()
        .sub(user.id.clone())
        .sub_id(user.id.clone())
        .email(Some(user.email.clone()))
        .uuid(Some(uuid.clone()));

    let jwt = etc::jwt::jwt_config();
    let token = match jwt.generate_token(&claim) {
        Ok(token) => token,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    };

    let action = Action::new(
        user.email.clone(),
        ActionType::PasswordReset,
        60,
        uuid.clone(),
    );

    let first_name = user.first_name.clone().unwrap_or_default();
    let last_name = user.last_name.clone().unwrap_or_default();
    let sender = Smtp::new()
        .template(Template::ChangePasswordRequest)
        .to(user.email.clone())
        .name(Some(format_name(&first_name, &last_name)))
        .token(token)
        .build()
        .send();

    match sender {
        Ok(_) => {
            match service.create_action(action).await {
                Ok(action) => action,
                Err(e) => {
                    return Err(ErrorResponse::from(HttpError::InternalServerError(
                        e.to_string(),
                    )))
                }
            };

            let message = etc::msg::MessageResponse::new(
                "Password reset request sent. Please check your email for the password reset link."
                    .to_string(),
                "password_reset".to_string(),
            );
            Ok(HttpResponse::Ok().json(web::Json(message)))
        }
        Err(e) => Err(ErrorResponse::from(HttpError::InternalServerError(
            e.to_string(),
        ))),
    }
}
