use crate::act;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc;
use crate::etc::http::messages::{MessageCode, MessageResponse};
use crate::fun::format_name;
use axum::Json;
use serde::{Deserialize, Serialize};
use smtp::{Smtp, Template};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ForgotPasswordRequestBody {
    pub email: String,
}

#[utoipa::path(
    post,
    path = "/account/credentials",
    tags = ["Account"],
    summary = "Initiate Password Reset",
    description = "Initiate a password reset request by providing the user's email address. If the email exists in the system, a password reset link will be sent to that email.",
    request_body = ForgotPasswordRequestBody,
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn post_credentials(
    Json(body): Json<ForgotPasswordRequestBody>,
) -> Result<Json<MessageResponse>, ErrorResponse> {
    let user = crate::db::get_user_by_username(&body.email)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::UserNotFound))?;

    if let Ok(Some(_)) = act::get_change_password_request(&user.email).await {
        return Ok(Json(MessageResponse::new(
            MessageCode::PasswordResetAlreadyRequested,
        )));
    }

    let sid = act::create_change_password_request(&user.email)
        .await
        .map_err(ErrorResponse::internal)?;

    let claim = jwt::Claims::default()
        .sub_id(user.id.clone())
        .email(user.email.clone())
        .sid(sid);

    let token = etc::auth::jwt::jwt_config()
        .generate_token(&claim)
        .map_err(ErrorResponse::internal)?;

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);

    Smtp::new()
        .template(Template::ChangePasswordRequest)
        .to(user.email.clone())
        .name(Some(name))
        .token(token)
        .build()
        .and_then(|smtp| smtp.send())
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(
        MessageCode::PasswordResetRequested,
    )))
}
