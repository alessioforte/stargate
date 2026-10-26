use super::SignupRequestBody;
use crate::{
    act::{
        self, PendingSignupProfile, SignupOrigin,
        email_throttle::{EmailFlow, check_and_record_email_request},
    },
    err::{ErrorCode, ErrorResponse},
    web::{
        links::{login_url, signup_url},
        messages::{MessageCode, MessageResponse},
    },
};
use axum::Json;
use db::ent::canonical_email;
use smtp::{Smtp, Template};

#[utoipa::path(
    post,
    path = "/signup",
    tags = ["Signup"],
    description = "Start a signup for an email address (`SIGNUP_MODE=open` only). The response is the same whether or not the address already has an account: a new address receives a verification link, an existing account receives a sign-in reminder, and a pending signup has its link resent.",
    request_body = SignupRequestBody,
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 403, description = "The email domain is not in `SIGNUP_ALLOWED_DOMAINS` (`signup.domain_not_allowed`)", body = ErrorResponse),
        (status = 404, description = "Self-registration is disabled (`SIGNUP_MODE` is not `open`, `feature.disabled`)", body = ErrorResponse),
        (status = 429, description = "Too many emails requested for this address", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn post_signup(
    Json(body): Json<SignupRequestBody>,
) -> Result<Json<MessageResponse>, ErrorResponse> {
    let email = canonical_email(&body.email);
    if !smtp::is_valid_address(&email) {
        return Err(ErrorResponse::new(ErrorCode::RequestInvalid).with_param("field", "email"));
    }
    // Depends on the domain only, never on an existing account.
    super::ensure_self_service(&email)?;
    check_and_record_email_request(EmailFlow::Signup, &email).await?;

    let email_kind = if crate::data::db::get_user_by_email(&email)
        .await
        .map_err(ErrorResponse::internal)?
        .is_some()
    {
        SignupEmail::AccountExists
    } else {
        // Resend the pending signup's link rather than refusing: the
        // per-recipient throttle bounds repeats, and a lost email stays
        // recoverable.
        let sid = match act::check_email_verification_request(&email)
            .await
            .map_err(ErrorResponse::internal)?
        {
            Some(sid) => sid,
            None => act::create_email_verification_request(&PendingSignupProfile::self_service(
                email.clone(),
            ))
            .await
            .map_err(ErrorResponse::internal)?,
        };
        SignupEmail::Verify { sid }
    };

    // Same response either way, and delivery happens in the background, so
    // neither the body nor the response time reveals an existing account.
    spawn_signup_email(email, email_kind);
    Ok(Json(MessageResponse::new(MessageCode::SignupRequested)))
}

/// Admin invitations: conflicts are reported and delivery completes before
/// the response, so the inviting admin learns about every failure.
pub(crate) async fn send_invitation(profile: PendingSignupProfile) -> Result<(), ErrorResponse> {
    let profile = PendingSignupProfile {
        origin: SignupOrigin::Invitation,
        email: canonical_email(&profile.email),
        ..profile
    };
    if crate::data::db::get_user_by_email(&profile.email)
        .await
        .map_err(ErrorResponse::internal)?
        .is_some()
    {
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

    let email = profile.email.clone();
    let delivery_sid = sid.clone();
    let delivered = tokio::task::spawn_blocking(move || {
        send_signup_email(&email, SignupEmail::Verify { sid: delivery_sid })
    })
    .await
    .map_err(ErrorResponse::internal)
    .and_then(|result| result.map_err(ErrorResponse::internal));
    if let Err(error) = delivered {
        let _ = act::delete_email_verification_request(&sid).await;
        return Err(error);
    }

    Ok(())
}

enum SignupEmail {
    /// A verification link for the pending signup `sid`.
    Verify { sid: String },
    /// A sign-in reminder for an address that already has an account.
    AccountExists,
}

fn spawn_signup_email(email: String, kind: SignupEmail) {
    std::mem::drop(tokio::task::spawn_blocking(move || {
        if let Err(error) = send_signup_email(&email, kind) {
            tracing::error!(%error, "failed to send signup email");
        }
    }));
}

fn send_signup_email(email: &str, kind: SignupEmail) -> anyhow::Result<()> {
    let (template, link) = match kind {
        SignupEmail::Verify { sid } => {
            let claim = jwt::Claims::default()
                .subject("signup_request".to_string())
                .sid(sid)
                .email(email.to_string());
            let token = crate::etc::auth::jwt::jwt_config().generate_token(&claim)?;
            (Template::SignupRequest, signup_url(&token))
        }
        SignupEmail::AccountExists => (Template::SignupAccountExists, login_url()),
    };

    Smtp::new()
        .template(template)
        .to(email.to_string())
        .link(link)
        .build()
        .and_then(|smtp| smtp.send())?;
    Ok(())
}
