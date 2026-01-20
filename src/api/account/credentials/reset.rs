use super::ChangePasswordRequestBody;
use crate::act;
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::fun::format_name;
use actix_web::{HttpMessage, HttpRequest, HttpResponse, put, web};
use db::ent::AuditContext;
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
        (status = 200, description = "OK"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal Server Error")
    )
)]
#[put("")]
pub async fn handler(
    req: HttpRequest,
    body: web::Json<ChangePasswordRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

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
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            // TODO: different error?
            "Request not found".to_string(),
        )));
    }

    let email = email.unwrap();
    let token_sid = claims.sid.clone().unwrap_or_default();

    let sid = match act::get_change_password_request(&email).await {
        Ok(pra) => pra,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
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
    let password = Hash::encode(&body.password).unwrap();

    let ctx = ctx.with_account_id(user.account_id);

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

    let message = etc::msg::MessageResponse::new(
        "Change Password".to_string(),
        "change_password".to_string(),
    );

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();

    let sender = Smtp::new()
        .template(Template::PasswordChangedNotification)
        .to(user.email.clone())
        .name(Some(format_name(&given_name, &family_name)))
        .token(token)
        .build()
        .send();

    match sender {
        Ok(_) => {}
        Err(e) => error!("Could not send email: {:?}", e),
    }

    Ok(HttpResponse::Ok().json(web::Json(message)))
}
