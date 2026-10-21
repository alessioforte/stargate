use crate::act;
use crate::act::email_throttle::EmailFlow;
use crate::err::ErrorResponse;
use crate::etc;
use crate::etc::http::messages::{MessageCode, MessageResponse};
use crate::fun::format_name;
use axum::Json;
use db::ent::{User, canonical_email};
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
    description = "Request a password-reset link for an email address. The response is the same whether or not an account uses the address. While a reset request is pending, a new request resends its link.",
    request_body = ForgotPasswordRequestBody,
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 429, description = "Too many emails requested for this address", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn post_credentials(
    Json(body): Json<ForgotPasswordRequestBody>,
) -> Result<Json<MessageResponse>, ErrorResponse> {
    act::email_throttle::check_and_record_email_request(EmailFlow::PasswordReset, &body.email)
        .await?;

    // Only an exact email match may receive a link: a nickname or phone number
    // would select a recipient outside the per-recipient throttle's bucket.
    let user = crate::db::get_user_by_username(&body.email)
        .await
        .map_err(ErrorResponse::internal)?
        .filter(|user| user.email == canonical_email(&body.email));

    if let Some(user) = user {
        // Reuse the pending request so a lost or failed email can be resent;
        // the per-recipient throttle bounds the repeats.
        let sid = match act::get_change_password_request(&user.email)
            .await
            .map_err(ErrorResponse::internal)?
        {
            Some(sid) => sid,
            None => act::create_change_password_request(&user.email)
                .await
                .map_err(ErrorResponse::internal)?,
        };
        spawn_reset_email(user, sid);
    }

    // Same response and no delivery wait either way, so neither the body nor
    // the response time reveals whether the address has an account.
    Ok(Json(MessageResponse::new(
        MessageCode::PasswordResetRequested,
    )))
}

fn spawn_reset_email(user: User, sid: String) {
    std::mem::drop(tokio::task::spawn_blocking(move || {
        if let Err(error) = send_reset_email(&user, sid) {
            tracing::error!(%error, user_id = %user.id, "failed to send password-reset email");
        }
    }));
}

fn send_reset_email(user: &User, sid: String) -> anyhow::Result<()> {
    let claim = jwt::Claims::default()
        .sub_id(user.id.clone())
        .email(user.email.clone())
        .sid(sid);
    let token = etc::auth::jwt::jwt_config().generate_token(&claim)?;

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    Smtp::new()
        .template(Template::ChangePasswordRequest)
        .to(user.email.clone())
        .name(Some(format_name(&given_name, &family_name)))
        .token(token)
        .build()
        .and_then(|smtp| smtp.send())?;
    Ok(())
}
