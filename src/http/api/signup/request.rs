use super::SignupRequestBody;
use crate::{
    act,
    err::{ErrorResponse, HttpError},
    etc::msg::MessageResponse,
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
    let user = crate::db::get_user_by_username(&body.email)
        .await
        .map_err(ErrorResponse::internal)?;
    if user.is_some() {
        return Err(ErrorResponse::from(HttpError::Conflict(
            "User already exists".to_string(),
        )));
    }

    let pending = act::check_email_verification_request(&body.email)
        .await
        .map_err(ErrorResponse::internal)?;
    if pending.is_some() {
        return Err(ErrorResponse::from(HttpError::Conflict(
            "Signup request already exists".to_string(),
        )));
    }

    let sid = act::create_email_verification_request(&body.email)
        .await
        .map_err(ErrorResponse::internal)?;

    let claim = jwt::Claims::default()
        .subject("signup_request".to_string())
        .sid(sid)
        .email(body.email.clone());

    let token = crate::etc::jwt::jwt_config()
        .generate_token(&claim)
        .map_err(ErrorResponse::internal)?;

    Smtp::new()
        .template(Template::SignupRequest)
        .to(body.email.clone())
        .token(token)
        .build()
        .and_then(|smtp| smtp.send())
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(
        "A signup request has been sent to your email. Please check your inbox.",
        "signup_request",
    )))
}
