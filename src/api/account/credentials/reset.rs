use super::ChangePasswordRequestBody;
use crate::act;
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::etc::msg::MessageResponse;
use crate::etc::reqctx::take_audit_context_from;
use crate::fun::format_name;
use axum::Json;
use axum::extract::{FromRequest, Request};
use smtp::{Smtp, Template};
use tracing::error;

#[utoipa::path(
    put,
    path = "/account/credentials",
    tags = ["Account"],
    summary = "Change Password",
    description = "Change the user's password using a valid password reset token.",
    request_body = ChangePasswordRequestBody,
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn put_credentials(mut req: Request) -> Result<Json<MessageResponse>, ErrorResponse> {
    let ctx = take_audit_context_from(req.extensions_mut());

    let Json(body) = Json::<ChangePasswordRequestBody>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;

    let token = body.token.clone();
    let claims = etc::jwt::jwt_config()
        .validate_token(&token)
        .map_err(|_| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;

    let email = claims.email.clone().ok_or_else(|| {
        ErrorResponse::from(HttpError::BadRequest(
            "Token missing email claim".to_string(),
        ))
    })?;
    let token_sid = claims.sid.clone().unwrap_or_default();

    let stored_sid = act::get_change_password_request(&email)
        .await
        .map_err(ErrorResponse::internal)?;

    if stored_sid.as_deref() != Some(&token_sid) {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid or expired password reset request".to_string(),
        )));
    }

    let user = crate::db::get_user_by_username(&email)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::DocumentNotFound("User not found".to_string()))
        })?;

    act::password_policy::validate_for_user(&user, &body.password)
        .await
        .map_err(|m| ErrorResponse::from(HttpError::BadRequest(m)))?;

    let password = crate::etc::pw::hash_password(body.password.clone())
        .await
        .ok_or_else(|| ErrorResponse::internal("failed to hash password"))?;

    crate::db::change_password(&user.id, &password, ctx)
        .await
        .map_err(|e| {
            error!("Could not update password: {:?}", e);
            ErrorResponse::from(HttpError::InternalServerError(
                "Could not update password".to_string(),
            ))
        })?;

    let _ = act::delete_change_password_request(&email).await;

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    if let Err(e) = Smtp::new()
        .template(Template::PasswordChangedNotification)
        .to(user.email.clone())
        .name(Some(format_name(&given_name, &family_name)))
        .token(token)
        .build()
        .and_then(|smtp| smtp.send())
    {
        error!("Could not send email: {:?}", e);
    }

    Ok(Json(MessageResponse::new(
        "Password changed successfully",
        "password_changed",
    )))
}
