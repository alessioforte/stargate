use super::ChangePasswordRequestBody;
use crate::ctx::audit_request_from;
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::user::profile::{format_name, locale as user_locale};
use crate::iam::{auth::jwt::jwt_config, pwd};
use crate::web::messages::{MessageCode, MessageWithContinuation};
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
        (status = 200, description = "OK, plus the app to continue on when the reset started there", body = MessageWithContinuation),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn put_credentials(req: Request) -> Result<Json<MessageWithContinuation>, ErrorResponse> {
    let audit_request = audit_request_from(req.extensions());

    let Json(body) = Json::<ChangePasswordRequestBody>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;

    let token = body.token.clone();
    let claims = jwt_config()
        .validate_password_reset_token(&token)
        .map_err(|_| ErrorResponse::new(ErrorCode::PasswordResetTokenInvalid))?;

    let email = claims
        .email
        .clone()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::PasswordResetTokenInvalid))?;
    let token_sid = claims.sid.clone().unwrap_or_default();

    let stored = pwd::reset::get_change_password_request(&email)
        .await
        .map_err(ErrorResponse::internal)?
        .filter(|request| request.sid == token_sid);

    let Some(stored) = stored else {
        return Err(ErrorResponse::new(ErrorCode::PasswordResetRequestInvalid));
    };

    let user = crate::data::db::get_user_by_email(&email)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::UserNotFound))?;

    if claims.sub_id.as_deref() != Some(user.id.as_str()) {
        return Err(ErrorResponse::new(ErrorCode::PasswordResetTokenInvalid));
    }

    pwd::policy::validate_for_user(&user, &body.password).await?;

    // The app the reset began in still has to be enabled and keep its endpoint
    // (OpenID Connect Core 1.0 §4).
    let continuation =
        crate::iam::oidc::continuation::complete(stored.continuation.as_ref(), &user.email)
            .await
            .map_err(ErrorResponse::internal)?;

    let password = pwd::hash::hash_password(body.password.clone()).await?;

    // Consume the request before writing: failed or concurrent consumption must
    // never leave a successful password change with a reusable reset link.
    if !pwd::reset::delete_change_password_request(&email)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Err(ErrorResponse::new(ErrorCode::PasswordResetRequestInvalid));
    }

    let audit_context = db::ent::TrustedAuditContext::application(
        db::ent::TrustedAuditActor::user(&user.id),
        audit_request,
    );
    crate::data::db::change_password(&user.id, &password, audit_context)
        .await
        .map_err(|e| {
            error!("Could not update password: {:?}", e);
            ErrorResponse::new(ErrorCode::PasswordUpdateFailed)
        })?;

    crate::iam::sess::registry::revoke_all_sessions(&user.id).await?;

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let locale = user_locale(&user).unwrap_or_else(smtp::default_locale);
    tokio::task::spawn_blocking(move || {
        if let Err(e) = Smtp::new()
            .template(Template::PasswordChangedNotification)
            .locale(locale)
            .to(user.email.clone())
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
        Some(continuation) => MessageWithContinuation::new(MessageCode::PasswordChanged)
            .with_app_redirect(continuation.url, continuation.client_name),
        None => MessageWithContinuation::new(MessageCode::PasswordChanged),
    }))
}

#[cfg(all(test, feature = "memory", feature = "sqlite"))]
mod tests {
    use super::put_credentials;
    use crate::data::db::with_test_database;
    use crate::iam::oidc::continuation::LoginContinuation;
    use crate::iam::{auth::jwt::jwt_config, pwd};
    use axum::body::Body;
    use db::ent::{CredentialType, OAuthClient, Profile, TrustedAuditActor, TrustedAuditContext};
    use http::{Method, Request, header::CONTENT_TYPE};

    const PASSWORD: &str = "Str0ng-Passw0rd!";

    async fn user(email: &str, nickname: &str) -> String {
        let context = TrustedAuditContext::application(
            TrustedAuditActor::anonymous(),
            db::ent::TrustedAuditRequest::from_http(
                ulid::Ulid::generate().to_string(),
                None,
                None,
                None,
            ),
        );
        crate::data::db::create_user(
            Profile::new(email.to_string(), nickname.to_string()),
            CredentialType::Password,
            "TEST_PASSWORD_HASH",
            None,
            context,
        )
        .await
        .expect("create user")
        .id
    }

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

    async fn app_client(client_id: &str, enabled: bool, initiate_login_uri: Option<&str>) {
        let mut client = OAuthClient::new(
            client_id.to_string(),
            None,
            "Orders".to_string(),
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

    async fn change_password(
        user_id: &str,
        email: &str,
        sid: &str,
    ) -> crate::web::messages::MessageWithContinuation {
        let claim = jwt::Claims::default()
            .sub_id(user_id.to_string())
            .email(email.to_string())
            .sid(sid.to_string());
        let token = jwt_config()
            .generate_password_reset_token(claim)
            .expect("reset token");
        let body = serde_json::json!({ "token": token, "password": PASSWORD });
        let request = Request::builder()
            .method(Method::PUT)
            .uri("/account/credentials")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .expect("request");
        put_credentials(request).await.expect("password changes").0
    }

    #[tokio::test]
    async fn an_app_started_reset_continues_there_after_the_interaction_expires() {
        with_test_database(async {
            app_client("orders", true, Some("https://orders.example/login")).await;
            let email = "ada@example.com";
            let user_id = user(email, "ada").await;
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
            let sid = pwd::reset::issue_change_password_request(email, continuation)
                .await
                .expect("pending reset");

            let response = change_password(&user_id, email, &sid).await;

            assert_eq!(
                response.message.code,
                crate::web::messages::MessageCode::PasswordChanged
            );
            assert_eq!(response.client_name.as_deref(), Some("Orders"));
            let url = url::Url::parse(response.continue_url.as_deref().expect("continue url"))
                .expect("absolute endpoint");
            let params = url
                .query_pairs()
                .collect::<std::collections::HashMap<_, _>>();
            assert_eq!(
                params.get("login_hint").map(|value| value.as_ref()),
                Some(email)
            );
        })
        .await;
    }

    #[tokio::test]
    async fn a_reset_without_an_app_ends_on_the_login_page() {
        with_test_database(async {
            let email = "grace@example.com";
            let user_id = user(email, "grace").await;
            let sid = pwd::reset::issue_change_password_request(email, None)
                .await
                .expect("pending reset");

            let response = change_password(&user_id, email, &sid).await;

            assert_eq!(response.continue_url, None);
            assert_eq!(response.client_name, None);
        })
        .await;
    }

    #[tokio::test]
    async fn a_client_disabled_before_the_reset_completes_has_no_continuation() {
        with_test_database(async {
            app_client("orders", true, Some("https://orders.example/login")).await;
            let email = "edsger@example.com";
            let user_id = user(email, "edsger").await;
            let sid = pwd::reset::issue_change_password_request(
                email,
                Some(LoginContinuation::new("orders", None)),
            )
            .await
            .expect("pending reset");
            crate::data::db::set_oauth_client_enabled("orders", false, admin_context())
                .await
                .expect("disable client");

            let response = change_password(&user_id, email, &sid).await;

            assert_eq!(response.continue_url, None);
        })
        .await;
    }
}
