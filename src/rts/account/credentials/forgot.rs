use crate::act;
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::fun::format_name;
use actix_web::{HttpResponse, post, web};
use serde::{Deserialize, Serialize};
use smtp::{Smtp, Template};
use utoipa::ToSchema;
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

    let user = match crate::db::get_user_by_username(&body.email).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();

    let uuid = act::create_change_password_request(&user.email)
        .await
        .map_err(|e| {
            ErrorResponse::from(HttpError::InternalServerError(format!(
                "Failed to create change password request: {}",
                e
            )))
        })?;

    let claim = jwt::Claims::default()
        .subject(user.id.clone())
        .sub_id(user.id.clone())
        .email(user.email.clone())
        .uuid(uuid.clone());

    let jwt = etc::jwt::jwt_config();
    let token = match jwt.generate_token(&claim) {
        Ok(token) => token,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let sender = Smtp::new()
        .template(Template::ChangePasswordRequest)
        .to(user.email.clone())
        .name(Some(format_name(&given_name, &family_name)))
        .token(token)
        .build()
        .send();

    match sender {
        Ok(_) => {
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
