use super::ChangePasswordRequestBody;
use crate::act;
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::etc::reqctx::take_audit_context;
use crate::fun::format_name;
use actix_web::{HttpRequest, HttpResponse, put, web};
use etc::msg::MessageResponse;
use pw::Hash;
use smtp::{Smtp, Template};
use tracing::error;

#[utoipa::path(
    context_path = "/account",
    path = "/credentials",
    tags = ["Account"],
    summary = "Change Password",
    description = "Change the user's password using a valid password reset token.",
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[put("")]
pub async fn handler(
    req: HttpRequest,
    body: web::Json<ChangePasswordRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = take_audit_context(&req);

    let body = body.into_inner();
    let token = body.token.clone();
    let jwt = etc::jwt::jwt_config();
    let claims = match jwt.validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )));
        }
    };

    let email = claims.email.clone();
    if email.is_none() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Token missing email claim".to_string(),
        )));
    }

    let email = email.unwrap();
    let token_sid = claims.sid.clone().unwrap_or_default();

    let sid = match act::get_change_password_request(&email).await {
        Ok(pra) => pra,
        Err(e) => {
            return Err(ErrorResponse::internal(e));
        }
    };

    if sid.is_none() || sid.unwrap() != token_sid {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid or expired password reset request".to_string(),
        )));
    }

    let user = match crate::db::get_user_by_username(&email).await {
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
    let password = Hash::encode(&body.password).unwrap();

    match crate::db::change_password(&user.id, &password, ctx).await {
        Ok(_) => {}
        Err(e) => {
            error!("Could not update password: {:?}", e);
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                "Could not update password".to_string(),
            )));
        }
    }

    let _ = act::delete_change_password_request(&email).await;

    let message =
        etc::msg::MessageResponse::new("Password changed successfully", "password_changed");

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();

    let sender = Smtp::new()
        .template(Template::PasswordChangedNotification)
        .to(user.email.clone())
        .name(Some(format_name(&given_name, &family_name)))
        .token(token)
        .build()
        .and_then(|smtp| smtp.send());

    match sender {
        Ok(_) => {}
        Err(e) => error!("Could not send email: {:?}", e),
    }

    Ok(HttpResponse::Ok().json(web::Json(message)))
}
