use super::SignupCompleteRequestBody;
use crate::ctx::audit_request_from;
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::user::{
    email_verification, nicknames,
    profile::{format_name, with_locale},
    signup,
};
use crate::iam::{auth::jwt::jwt_config, pwd};
use crate::web::messages::{MessageCode, MessageWithContinuation};
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
        (status = 200, description = "OK, plus the app to continue on when the signup started there", body = MessageWithContinuation),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 403, description = "The email domain is no longer allowed (`signup.domain_not_allowed`)", body = ErrorResponse),
        (status = 404, description = "Not Found, or disabled by `SIGNUP_MODE` (`feature.disabled`)", body = ErrorResponse),
        (status = 409, description = "Conflict", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn put_signup(req: Request) -> Result<Json<MessageWithContinuation>, ErrorResponse> {
    let audit_request = audit_request_from(req.extensions());

    let Json(body) = Json::<SignupCompleteRequestBody>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;

    let jwt_cfg = jwt_config();
    let claim = jwt_cfg
        .validate_signup_token(&body.token)
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::SignupTokenInvalid).with_message(error.to_string())
        })?;

    if claim.sub != "signup" {
        return Err(ErrorResponse::new(ErrorCode::SignupTokenInvalid));
    }

    let sid = claim
        .sub_id
        .clone()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::SignupTokenInvalid))?;

    let signup = signup::get_signup_request(&sid)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::SignupRequestNotFound))?;

    if let Some(ref claim_email) = claim.email
        && &signup.email != claim_email
    {
        return Err(ErrorResponse::new(ErrorCode::SignupEmailMismatch));
    }
    super::ensure_completable(&signup)?;

    if crate::data::db::get_user_by_email(&signup.email)
        .await
        .map_err(ErrorResponse::internal)?
        .is_some()
    {
        return Err(ErrorResponse::new(ErrorCode::SignupUserAlreadyExists));
    }

    // The app the flow began in still has to be enabled and keep its endpoint
    // (OpenID Connect Core 1.0 §4).
    let continuation =
        crate::iam::oidc::continuation::complete(signup.continuation.as_ref(), &signup.email)
            .await
            .map_err(ErrorResponse::internal)?;

    let nickname = if body.nickname.trim().is_empty() {
        signup.email.clone()
    } else {
        body.nickname.trim().to_string()
    };

    nicknames::ensure_nickname_available(&nickname, &signup.email, None).await?;

    let organization = match signup.membership.as_ref() {
        Some(membership) => Some(
            crate::data::db::get_organization_by_id(&membership.organization_id)
                .await
                .map_err(ErrorResponse::internal)?
                .ok_or_else(|| {
                    ErrorResponse::new(ErrorCode::OrganizationNotFound)
                        .with_param("id", membership.organization_id.clone())
                })?,
        ),
        None => None,
    };
    let password_validation = match organization.as_ref() {
        Some(organization) => pwd::policy::validate_for_organization(
            organization,
            &body.password,
            Some(&nickname),
            Some(&signup.email),
        ),
        None => pwd::policy::validate_global(&body.password, Some(&nickname), Some(&signup.email)),
    };
    password_validation.map_err(|message| {
        ErrorResponse::new(ErrorCode::PasswordPolicyViolation).with_message(message)
    })?;

    let locale = signup.locale;
    let profile = Profile::new(signup.email.clone(), nickname)
        .given_name(optional_nonempty(&body.given_name))
        .family_name(optional_nonempty(&body.family_name))
        .phone_number(body.phone_number.clone())
        .picture(signup.picture)
        .attrs(with_locale(signup.attrs, locale));

    let password = pwd::hash::hash_password(body.password.clone()).await?;

    let audit_context =
        TrustedAuditContext::application(TrustedAuditActor::anonymous(), audit_request);
    let user = crate::data::db::create_user(
        profile,
        CredentialType::Password,
        &password,
        signup.membership,
        audit_context,
    )
    .await
    .map_err(|error| {
        if db::is_unique_violation(&error) {
            ErrorResponse::new(ErrorCode::SignupUserAlreadyExists)
        } else {
            ErrorResponse::internal(error)
        }
    })?;

    let _ = signup::delete_signup_request(&sid).await;
    let _ = email_verification::delete_email_verification_request(&sid).await;

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = Smtp::new()
            .template(smtp::Template::SignupCompleted)
            .locale(locale)
            .to(user.email)
            .name(Some(format_name(&given_name, &family_name)))
            .build()
            .and_then(|smtp| smtp.send())
        {
            error!("Could not send email: {:?}", e);
        }
    })
    .await
    .map_err(ErrorResponse::internal)?;

    Ok(Json(match continuation {
        Some(continuation) => MessageWithContinuation::new(MessageCode::SignupCompleted)
            .with_app_redirect(continuation.url, continuation.client_name),
        None => MessageWithContinuation::new(MessageCode::SignupCompleted),
    }))
}

fn optional_nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(all(test, feature = "memory", feature = "sqlite"))]
mod tests {
    use super::put_signup;
    use crate::data::db::with_test_database;
    use crate::iam::oidc::continuation::LoginContinuation;
    use crate::iam::user::{email_verification, signup};
    use crate::iam::{
        auth::jwt::jwt_config,
        user::signup::{PendingSignupProfile, SignupOrigin},
    };
    use crate::web::messages::MessageWithContinuation;
    use axum::body::Body;
    use db::ent::{OAuthClient, TrustedAuditContext};
    use http::{Method, Request, header::CONTENT_TYPE};
    use tools::Locale;

    const PASSWORD: &str = "Str0ng-Passw0rd!";

    fn admin_context() -> TrustedAuditContext {
        TrustedAuditContext::admin_control_plane(
            db::ent::TrustedAdminActor::admin("01JZ000000000000000000000A"),
            db::ent::TrustedAuditRequest::from_http(
                ulid::Ulid::generate().to_string(),
                None,
                None,
                None,
            ),
        )
    }

    async fn app_client(
        client_id: &str,
        name: &str,
        enabled: bool,
        initiate_login_uri: Option<&str>,
    ) {
        let mut client = OAuthClient::new(
            client_id.to_string(),
            None,
            name.to_string(),
            None,
            "none".to_string(),
            vec!["authorization_code".to_string()],
            vec!["code".to_string()],
            vec!["https://orders.example/callback".to_string()],
            vec!["openid".to_string()],
            Vec::new(),
            serde_json::json!({}),
        );
        client.enabled = enabled;
        client.initiate_login_uri = initiate_login_uri.map(str::to_string);
        crate::data::db::create_oauth_client(client, admin_context())
            .await
            .expect("create client");
    }

    async fn pending_signup(email: &str, continuation: Option<LoginContinuation>) -> String {
        // Invitations complete in the default signup mode; a self-service
        // signup proceeds only while `SIGNUP_MODE=open` (see `request`).
        let profile = PendingSignupProfile {
            origin: SignupOrigin::Invitation,
            continuation,
            ..PendingSignupProfile::self_service(email, Locale::En)
        };
        let sid = email_verification::create_email_verification_request(&profile)
            .await
            .expect("pending email verification");
        signup::store_signup_request(&sid, &profile)
            .await
            .expect("pending signup");
        sid
    }

    async fn complete_signup(sid: &str) -> MessageWithContinuation {
        let claim = jwt::Claims::default()
            .subject("signup".to_string())
            .sub_id(sid.to_string())
            .email(
                signup::get_signup_request(sid)
                    .await
                    .unwrap()
                    .unwrap()
                    .email,
            );
        let token = jwt_config()
            .generate_signup_token(claim)
            .expect("signup token");
        let body = serde_json::json!({
            "token": token,
            "givenName": "Ada",
            "familyName": "Lovelace",
            "nickname": "",
            "password": PASSWORD,
            "phoneNumber": null,
        });
        let request = Request::builder()
            .method(Method::PUT)
            .uri("/signup")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .expect("request");
        put_signup(request).await.expect("signup completes").0
    }

    #[tokio::test]
    async fn a_signup_that_started_in_an_app_continues_there() {
        with_test_database(async {
            app_client(
                "orders",
                "Orders",
                true,
                Some("https://orders.example/login"),
            )
            .await;
            let sid = pending_signup(
                "ada@example.com",
                Some(LoginContinuation::new(
                    "orders",
                    Some("https://orders.example/inbox".to_string()),
                )),
            )
            .await;

            let response = complete_signup(&sid).await;

            assert_eq!(
                response.message.code,
                crate::web::messages::MessageCode::SignupCompleted
            );
            assert_eq!(response.client_name.as_deref(), Some("Orders"));
            let url = url::Url::parse(response.continue_url.as_deref().expect("continue url"))
                .expect("absolute endpoint");
            let params = url
                .query_pairs()
                .collect::<std::collections::HashMap<_, _>>();
            assert_eq!(
                params.get("iss").map(|value| value.as_ref()),
                Some(jwt::issuer_from_env().as_str())
            );
            assert_eq!(
                params.get("login_hint").map(|value| value.as_ref()),
                Some("ada@example.com")
            );
            assert_eq!(
                params.get("target_link_uri").map(|value| value.as_ref()),
                Some("https://orders.example/inbox")
            );
        })
        .await;
    }

    #[tokio::test]
    async fn a_signup_without_an_app_ends_on_the_login_page() {
        with_test_database(async {
            let sid = pending_signup("grace@example.com", None).await;

            let response = complete_signup(&sid).await;

            assert_eq!(response.continue_url, None);
            assert_eq!(response.client_name, None);
        })
        .await;
    }

    #[tokio::test]
    async fn the_link_works_in_another_tab_and_after_the_interaction_expires() {
        with_test_database(async {
            app_client(
                "orders",
                "Orders",
                true,
                Some("https://orders.example/login"),
            )
            .await;
            let query = crate::iam::oidc::request::AuthorizationRequest {
                client_id: Some("orders".to_string()),
                ..Default::default()
            };
            let interaction = crate::iam::oidc::interactions::park(query)
                .await
                .expect("parked request");
            let continuation = crate::iam::oidc::continuation::from_interaction(Some(&interaction))
                .await
                .expect("resolve continuation");
            // The parked request is gone by the time the emailed link is used.
            crate::iam::oidc::interactions::take(&interaction)
                .await
                .expect("take interaction")
                .expect("parked request");
            let sid = pending_signup("linus@example.com", continuation).await;

            let response = complete_signup(&sid).await;

            assert!(
                response.continue_url.is_some(),
                "the continuation lives on the pending signup"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn a_client_disabled_before_completion_has_no_continuation() {
        with_test_database(async {
            app_client(
                "orders",
                "Orders",
                true,
                Some("https://orders.example/login"),
            )
            .await;
            let sid = pending_signup(
                "edsger@example.com",
                Some(LoginContinuation::new("orders", None)),
            )
            .await;
            crate::data::db::set_oauth_client_enabled("orders", false, admin_context())
                .await
                .expect("disable client");

            let response = complete_signup(&sid).await;

            assert_eq!(response.continue_url, None);
        })
        .await;
    }

    #[tokio::test]
    async fn an_endpoint_written_outside_the_admin_api_has_no_continuation() {
        with_test_database(async {
            app_client("dangerous", "Dangerous", true, Some("javascript:alert(1)")).await;
            let sid = pending_signup(
                "ada@example.com",
                Some(LoginContinuation::new("dangerous", None)),
            )
            .await;

            let response = complete_signup(&sid).await;

            assert_eq!(response.continue_url, None);
            assert_eq!(response.client_name, None);
        })
        .await;
    }

    #[tokio::test]
    async fn an_invitation_keeps_the_app_the_administrator_chose() {
        with_test_database(async {
            app_client(
                "orders",
                "Orders",
                true,
                Some("https://orders.example/login"),
            )
            .await;
            app_client(
                "billing",
                "Billing",
                true,
                Some("https://billing.example/login"),
            )
            .await;
            let profile = PendingSignupProfile {
                origin: SignupOrigin::Invitation,
                continuation: Some(LoginContinuation::new("orders", None)),
                ..PendingSignupProfile::self_service("grace@example.com", Locale::En)
            };
            let sid = email_verification::create_email_verification_request(&profile)
                .await
                .expect("pending email verification");
            signup::store_signup_request(&sid, &profile)
                .await
                .expect("pending signup");
            // A self-service request for the same address names another app.
            email_verification::renew_email_verification_request(
                &sid,
                Some(LoginContinuation::new("billing", None)),
            )
            .await
            .expect("renew request");

            let response = complete_signup(&sid).await;

            assert_eq!(response.client_name.as_deref(), Some("Orders"));
        })
        .await;
    }

    const SELF_SERVICE_CHILD: &str = "STARGATE_TEST_SELF_SERVICE_CONTINUATION";

    /// A self-service signup completes only while `SIGNUP_MODE=open`, read once
    /// per process, so the child runs with that setting.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_self_service_signup_follows_the_last_app_its_link_was_requested_from() {
        if std::env::var_os(SELF_SERVICE_CHILD).is_some() {
            with_test_database(async {
                app_client(
                    "orders",
                    "Orders",
                    true,
                    Some("https://orders.example/login"),
                )
                .await;
                app_client(
                    "billing",
                    "Billing",
                    true,
                    Some("https://billing.example/login"),
                )
                .await;
                let profile = PendingSignupProfile {
                    origin: SignupOrigin::SelfService,
                    continuation: Some(LoginContinuation::new("orders", None)),
                    ..PendingSignupProfile::self_service("ada@example.com", Locale::En)
                };
                let sid = email_verification::create_email_verification_request(&profile)
                    .await
                    .expect("pending email verification");
                // The user opens the link: `GET /signup` stores the copy the
                // completion page reads.
                signup::store_signup_request(&sid, &profile)
                    .await
                    .expect("pending signup");
                // A later registration request for the same address starts in
                // another app.
                email_verification::renew_email_verification_request(
                    &sid,
                    Some(LoginContinuation::new("billing", None)),
                )
                .await
                .expect("renew request");

                let response = complete_signup(&sid).await;

                assert_eq!(response.client_name.as_deref(), Some("Billing"));
            })
            .await;
            return;
        }

        let output = tokio::task::spawn_blocking(move || {
            crate::conf::child_test::run(
                "api::signup::complete::tests::a_self_service_signup_follows_the_last_app_its_link_was_requested_from",
                |command| {
                    command
                        .env(SELF_SERVICE_CHILD, "1")
                        .env("SIGNUP_MODE", "open");
                },
            )
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
