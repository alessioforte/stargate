use super::SignupRequestBody;
use crate::{
    act::{self, PendingSignupProfile},
    err::{ErrorCode, ErrorResponse},
    etc::http::messages::{MessageCode, MessageResponse},
    fun::signup_url,
};
use axum::Json;
use smtp::{Smtp, Template};

#[utoipa::path(
    post,
    path = "/signup",
    tags = ["Signup"],
    request_body = SignupRequestBody,
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 409, description = "Conflict", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn post_signup(
    Json(body): Json<SignupRequestBody>,
) -> Result<Json<MessageResponse>, ErrorResponse> {
    send_signup_request(PendingSignupProfile::email_only(body.email)).await?;

    Ok(Json(MessageResponse::new(MessageCode::SignupRequested)))
}

pub(crate) async fn send_signup_request(
    profile: PendingSignupProfile,
) -> Result<(), ErrorResponse> {
    let user = crate::db::get_user_by_username(&profile.email)
        .await
        .map_err(ErrorResponse::internal)?;
    if user.is_some() {
        return Err(ErrorResponse::new(ErrorCode::SignupUserAlreadyExists));
    }

    let pending = act::check_email_verification_request(&profile.email)
        .await
        .map_err(ErrorResponse::internal)?;
    if pending.is_some() {
        return Err(ErrorResponse::new(ErrorCode::SignupRequestAlreadyExists));
    }

    let sid = act::create_email_verification_request(&profile)
        .await
        .map_err(ErrorResponse::internal)?;

    let claim = jwt::Claims::default()
        .subject("signup_request".to_string())
        .sid(sid.clone())
        .email(profile.email.clone());

    let token = crate::etc::auth::jwt::jwt_config()
        .generate_token(&claim)
        .map_err(ErrorResponse::internal)?;

    if let Err(error) = Smtp::new()
        .template(Template::SignupRequest)
        .to(profile.email)
        .link(signup_url(&token))
        .build()
        .and_then(|smtp| smtp.send())
    {
        let _ = act::delete_email_verification_request(&sid).await;
        return Err(ErrorResponse::internal(error));
    }

    Ok(())
}
