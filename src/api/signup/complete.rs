use super::SignupCompleteRequestBody;
use crate::act;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::msg::MessageResponse;
use crate::etc::reqctx::take_audit_context_from;
use crate::fun::format_name;
use axum::Json;
use axum::extract::{FromRequest, Request};
use db::ent::{CredentialType, Profile};
use pw::Hash;
use pw::{PasswordPolicy, PasswordPolicyValidator};
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
pub async fn put_signup(mut req: Request) -> Result<Json<MessageResponse>, ErrorResponse> {
    let ctx = take_audit_context_from(req.extensions_mut());

    let Json(body) = Json::<SignupCompleteRequestBody>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;

    let jwt_cfg = crate::etc::jwt::jwt_config();
    let claim = jwt_cfg
        .validate_token(&body.token)
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.to_string())))?;

    let sid = claim
        .sub_id
        .clone()
        .ok_or_else(|| ErrorResponse::from(HttpError::BadRequest("Invalid token".to_string())))?;

    let signup = act::get_signup_request(&sid)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::NotFound("Signup request not found".to_string()))
        })?;

    if let Some(ref claim_email) = claim.email
        && &signup != claim_email
    {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Email does not match".to_string(),
        )));
    }

    if crate::db::get_user_by_username(&signup)
        .await
        .map_err(ErrorResponse::internal)?
        .is_some()
    {
        return Err(ErrorResponse::from(HttpError::Conflict(
            "User already exists".to_string(),
        )));
    }

    if crate::db::get_user_by_username(&body.nickname)
        .await
        .map_err(ErrorResponse::internal)?
        .is_some()
    {
        return Err(ErrorResponse::from(HttpError::Conflict(
            "Nickname already exists".to_string(),
        )));
    }

    PasswordPolicy::standard()
        .validate(&body.password)
        .map_err(|m| ErrorResponse::from(HttpError::BadRequest(m)))?;

    let profile = Profile::new(signup, body.nickname.clone())
        .given_name(Some(body.given_name.clone()))
        .family_name(Some(body.family_name.clone()))
        .phone_number(body.phone_number.clone())
        .picture(None);

    let password = Hash::encode(&body.password).unwrap();

    let user = crate::db::create_user(profile, CredentialType::Password, &password, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    let _ = act::delete_signup_request(&sid).await;

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

    Ok(Json(MessageResponse::new(
        "User created successfully",
        "signup_completed",
    )))
}
