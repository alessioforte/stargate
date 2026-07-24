use super::ChangePasswordRequestBody;
use crate::act;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc;
use crate::etc::msg::{MessageCode, MessageResponse};
use crate::etc::reqctx::audit_request_from;
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
pub async fn put_credentials(req: Request) -> Result<Json<MessageResponse>, ErrorResponse> {
    let audit_request = audit_request_from(req.extensions());

    let Json(body) = Json::<ChangePasswordRequestBody>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;

    let token = body.token.clone();
    let claims = etc::jwt::jwt_config()
        .validate_token(&token)
        .map_err(|_| ErrorResponse::new(ErrorCode::PasswordResetTokenInvalid))?;

    let email = claims
        .email
        .clone()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::PasswordResetTokenInvalid))?;
    let token_sid = claims.sid.clone().unwrap_or_default();

    let stored_sid = act::get_change_password_request(&email)
        .await
        .map_err(ErrorResponse::internal)?;

    if stored_sid.as_deref() != Some(&token_sid) {
        return Err(ErrorResponse::new(ErrorCode::PasswordResetRequestInvalid));
    }

    let user = crate::db::get_user_by_username(&email)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::UserNotFound))?;

    act::password_policy::validate_for_user(&user, &body.password)
        .await
        .map_err(|message| {
            ErrorResponse::new(ErrorCode::PasswordPolicyViolation).with_message(message)
        })?;

    let password = crate::etc::pw::hash_password(body.password.clone())
        .await
        .ok_or_else(|| ErrorResponse::internal("failed to hash password"))?;

    let audit_context = db::ent::TrustedAuditContext::application(
        db::ent::TrustedAuditActor::user(&user.id),
        audit_request,
    );
    crate::db::change_password(&user.id, &password, audit_context)
        .await
        .map_err(|e| {
            error!("Could not update password: {:?}", e);
            ErrorResponse::new(ErrorCode::PasswordUpdateFailed)
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

    Ok(Json(MessageResponse::new(MessageCode::PasswordChanged)))
}
