use super::SignupCompleteRequestBody;
use crate::act;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::msg::{MessageCode, MessageResponse};
use crate::etc::reqctx::audit_request_from;
use crate::fun::format_name;
use axum::Json;
use axum::extract::{FromRequest, Request};
use db::ent::{CredentialType, Profile, TrustedAuditActor, TrustedAuditContext};
use smtp::Smtp;
use tracing::error;

#[utoipa::path(
    put,
    path = "/signup",
    tags = ["Signup"],
    request_body = SignupCompleteRequestBody,
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 404, description = "Not Found", body = ErrorResponse),
        (status = 409, description = "Conflict", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn put_signup(req: Request) -> Result<Json<MessageResponse>, ErrorResponse> {
    let audit_request = audit_request_from(req.extensions());

    let Json(body) = Json::<SignupCompleteRequestBody>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;

    let jwt_cfg = crate::etc::jwt::jwt_config();
    let claim = jwt_cfg.validate_token(&body.token).map_err(|error| {
        ErrorResponse::new(ErrorCode::SignupTokenInvalid).with_message(error.to_string())
    })?;

    if claim.sub != "signup" {
        return Err(ErrorResponse::new(ErrorCode::SignupTokenInvalid));
    }

    let sid = claim
        .sub_id
        .clone()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::SignupTokenInvalid))?;

    let signup = act::get_signup_request(&sid)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::SignupRequestNotFound))?;

    if let Some(ref claim_email) = claim.email
        && &signup.email != claim_email
    {
        return Err(ErrorResponse::new(ErrorCode::SignupEmailMismatch));
    }

    if crate::db::get_user_by_username(&signup.email)
        .await
        .map_err(ErrorResponse::internal)?
        .is_some()
    {
        return Err(ErrorResponse::new(ErrorCode::SignupUserAlreadyExists));
    }

    let nickname = if body.nickname.trim().is_empty() {
        signup.email.clone()
    } else {
        body.nickname.trim().to_string()
    };

    if crate::db::get_user_by_username(&nickname)
        .await
        .map_err(ErrorResponse::internal)?
        .is_some()
    {
        return Err(ErrorResponse::new(ErrorCode::UserNicknameAlreadyExists));
    }

    act::password_policy::validate_global(&body.password, Some(&nickname), Some(&signup.email))
        .map_err(|message| {
            ErrorResponse::new(ErrorCode::PasswordPolicyViolation).with_message(message)
        })?;

    let profile = Profile::new(signup.email.clone(), nickname)
        .given_name(optional_nonempty(&body.given_name))
        .family_name(optional_nonempty(&body.family_name))
        .phone_number(body.phone_number.clone())
        .picture(signup.picture)
        .attrs(signup.attrs);

    let password = crate::etc::pw::hash_password(body.password.clone())
        .await
        .ok_or_else(|| ErrorResponse::internal("failed to hash password"))?;

    let audit_context =
        TrustedAuditContext::application(TrustedAuditActor::anonymous(), audit_request);
    let user = crate::db::create_user(profile, CredentialType::Password, &password, audit_context)
        .await
        .map_err(ErrorResponse::internal)?;

    let _ = act::delete_signup_request(&sid).await;
    let _ = act::delete_email_verification_request(&sid).await;

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    if let Err(e) = Smtp::new()
        .template(smtp::Template::SignupCompleted)
        .to(user.email)
        .name(Some(format_name(&given_name, &family_name)))
        .build()
        .and_then(|smtp| smtp.send())
    {
        error!("Could not send email: {:?}", e);
    }

    Ok(Json(MessageResponse::new(MessageCode::SignupCompleted)))
}

fn optional_nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}
