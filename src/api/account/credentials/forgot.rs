use crate::act;
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::fun::format_name;
use actix_web::{HttpResponse, post, web};
use etc::msg::MessageResponse;
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
    tags = ["Account"],
    summary = "Initiate Password Reset",
    description = "Initiate a password reset request by providing the user's email address. If the email exists in the system, a password reset link will be sent to that email.",
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
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
            return Err(ErrorResponse::internal(e));
        }
    };

    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();

    let existing_request = act::get_change_password_request(&user.email).await;

    if let Ok(Some(_)) = existing_request {
        let message = MessageResponse::new(
            "A password reset request has already been sent to this email. Please check your email for the password reset link.",
            "password_reset_existing",
        );
        return Ok(HttpResponse::Ok().json(web::Json(message)));
    }

    let sid = act::create_change_password_request(&user.email)
        .await
        .map_err(|e| ErrorResponse::internal(e))?;

    let claim = jwt::Claims::default()
        .sub_id(user.id.clone())
        .email(user.email.clone())
        .sid(sid.clone());

    let jwt = etc::jwt::jwt_config();
    let token = match jwt.generate_token(&claim) {
        Ok(token) => token,
        Err(e) => {
            return Err(ErrorResponse::internal(e));
        }
    };

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);

    let sender = Smtp::new()
        .template(Template::ChangePasswordRequest)
        .to(user.email.clone())
        .name(Some(name))
        .token(token)
        .build()
        .and_then(|smtp| smtp.send());

    match sender {
        Ok(_) => {
            let message = etc::msg::MessageResponse::new(
                "Password reset request sent. Please check your email for the password reset link.",
                "password_reset",
            );
            Ok(HttpResponse::Ok().json(web::Json(message)))
        }
        Err(e) => Err(ErrorResponse::internal(e)),
    }
}
