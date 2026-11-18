use super::SignupRequestBody;
use crate::{
    err::{ErrorCode, ErrorResponse},
    iam::{
        auth::jwt::jwt_config,
        email_throttle::{EmailFlow, check_and_record_email_request},
        user::{
            email_verification, profile,
            signup::{PendingSignupProfile, SignupOrigin},
        },
    },
    web::{
        links::{login_url, signup_url},
        messages::{MessageCode, MessageResponse},
    },
};
use axum::Json;
use db::ent::canonical_email;
use http::HeaderMap;
use smtp::{Smtp, Template};
use tools::Locale;

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
    headers: HeaderMap,
    Json(body): Json<SignupRequestBody>,
) -> Result<Json<MessageResponse>, ErrorResponse> {
    let email = canonical_email(&body.email);
    if !smtp::is_valid_address(&email) {
        return Err(ErrorResponse::new(ErrorCode::RequestInvalid).with_param("field", "email"));
    }
    // Depends on the domain only, never on an existing account.
    super::ensure_self_service(&email)?;
    check_and_record_email_request(EmailFlow::Signup, &email).await?;

    // Resolved before the account lookup, so an existing address takes the
    // same path whether or not the flow started in an app.
    let continuation =
        crate::iam::oidc::continuation::from_interaction(body.interaction_id.as_deref())
            .await
            .map_err(ErrorResponse::internal)?;

    let requested = crate::web::request::accept_language(&headers);
    let default = smtp::default_locale();

    let (email_kind, locale) = if let Some(user) = crate::data::db::get_user_by_email(&email)
        .await
        .map_err(ErrorResponse::internal)?
    {
        (
            SignupEmail::AccountExists,
            profile::locale(&user).unwrap_or(default),
        )
    } else {
        // Resend the pending signup's link rather than refusing: the
        // per-recipient throttle bounds repeats, and a lost email stays
        // recoverable. Its language was chosen when the signup started.
        match email_verification::check_email_verification_request(&email)
            .await
            .map_err(ErrorResponse::internal)?
        {
            Some(sid) => {
                if let Some(continuation) = continuation {
                    email_verification::set_continuation(&sid, continuation)
                        .await
                        .map_err(ErrorResponse::internal)?;
                }
                let locale = email_verification::get_email_verification_request(&sid)
                    .await
                    .map_err(ErrorResponse::internal)?
                    .map(|pending| pending.locale)
                    .unwrap_or_else(|| profile::resolve_email_locale(None, requested, default));
                (SignupEmail::Verify { sid }, locale)
            }
            None => {
                let locale = profile::resolve_email_locale(None, requested, default);
                let profile = PendingSignupProfile {
                    continuation,
                    ..PendingSignupProfile::self_service(email.clone(), locale)
                };
                let sid = email_verification::create_email_verification_request(&profile)
                    .await
                    .map_err(ErrorResponse::internal)?;
                (SignupEmail::Verify { sid }, locale)
            }
        }
    };

    // Same response either way, and delivery happens in the background, so
    // neither the body nor the response time reveals an existing account.
    spawn_signup_email(email, email_kind, locale);
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

    let pending = email_verification::check_email_verification_request(&profile.email)
        .await
        .map_err(ErrorResponse::internal)?;
    if pending.is_some() {
        return Err(ErrorResponse::new(ErrorCode::SignupRequestAlreadyExists));
    }

    let sid = email_verification::create_email_verification_request(&profile)
        .await
        .map_err(ErrorResponse::internal)?;

    let email = profile.email.clone();
    let delivery_sid = sid.clone();
    let locale = profile.locale;
    let delivered = tokio::task::spawn_blocking(move || {
        send_signup_email(&email, SignupEmail::Verify { sid: delivery_sid }, locale)
    })
    .await
    .map_err(ErrorResponse::internal)
    .and_then(|result| result.map_err(ErrorResponse::internal));
    if let Err(error) = delivered {
        let _ = email_verification::delete_email_verification_request(&sid).await;
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

fn spawn_signup_email(email: String, kind: SignupEmail, locale: Locale) {
    std::mem::drop(tokio::task::spawn_blocking(move || {
        if let Err(error) = send_signup_email(&email, kind, locale) {
            tracing::error!(%error, "failed to send signup email");
        }
    }));
}

fn send_signup_email(email: &str, kind: SignupEmail, locale: Locale) -> anyhow::Result<()> {
    let (template, link) = match kind {
        SignupEmail::Verify { sid } => {
            let claim = jwt::Claims::default()
                .subject("signup_request".to_string())
                .sid(sid)
                .email(email.to_string());
            let token = jwt_config().generate_signup_verification_token(claim)?;
            (Template::SignupRequest, signup_url(&token))
        }
        SignupEmail::AccountExists => (Template::SignupAccountExists, login_url()),
    };

    Smtp::new()
        .template(template)
        .locale(locale)
        .to(email.to_string())
        .link(link)
        .build()
        .and_then(|smtp| smtp.send())?;
    Ok(())
}

#[cfg(all(test, feature = "memory", feature = "sqlite"))]
mod tests {
    use super::super::SignupRequestBody;
    use super::post_signup;
    use crate::data::db::with_test_database;
    use axum::Json;
    use db::ent::{
        CredentialType, Profile, TrustedAuditActor, TrustedAuditContext, TrustedAuditRequest,
    };
    use http::HeaderMap;

    const CHILD: &str = "STARGATE_TEST_SIGNUP_ENUMERATION";

    #[tokio::test]
    async fn signup_rejects_a_non_ascii_control_in_the_email_address() {
        let response = post_signup(
            HeaderMap::new(),
            Json(SignupRequestBody {
                email: "alice\u{0085}smith@example.com".to_string(),
                interaction_id: None,
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(response.status, http::StatusCode::BAD_REQUEST);
        assert_eq!(response.code, crate::err::ErrorCode::RequestInvalid);
        assert_eq!(response.params["field"], "email");
    }

    async fn send(email: &str, interaction_id: Option<&str>) -> serde_json::Value {
        let response = post_signup(
            HeaderMap::new(),
            Json(SignupRequestBody {
                email: email.to_string(),
                interaction_id: interaction_id.map(str::to_string),
            }),
        )
        .await
        .expect("signup requested");
        serde_json::to_value(response.0).expect("response json")
    }

    /// Self-registration needs `SIGNUP_MODE=open`, read once per process, so
    /// the child runs with that setting.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_signup_response_does_not_reveal_an_existing_account() {
        if std::env::var_os(CHILD).is_some() {
            with_test_database(async {
                let context = TrustedAuditContext::application(
                    TrustedAuditActor::anonymous(),
                    TrustedAuditRequest::from_http(
                        ulid::Ulid::generate().to_string(),
                        None,
                        None,
                        None,
                    ),
                );
                crate::data::db::create_user(
                    Profile::new("known@example.com".to_string(), "known".to_string()),
                    CredentialType::Password,
                    "TEST_PASSWORD_HASH",
                    None,
                    context,
                )
                .await
                .expect("create user");

                let known = send("known@example.com", None).await;
                let unknown = send("unknown@example.com", None).await;
                let no_interaction = send("unknown@example.com", Some("expired")).await;

                assert_eq!(known, unknown);
                assert_eq!(known, no_interaction);
                assert_eq!(known["code"], "signup.requested");
            })
            .await;
            return;
        }

        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    "api::signup::request::tests::the_signup_response_does_not_reveal_an_existing_account",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env("SIGNUP_MODE", "open")
                .output()
                .expect("run child test")
        })
        .await
        .expect("child test task");

        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
