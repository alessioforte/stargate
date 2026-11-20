use crate::err::ErrorResponse;
use crate::iam::email_throttle::{EmailFlow, check_and_record_email_request};
use crate::iam::user::profile::{format_name, locale as user_locale};
use crate::iam::{auth::jwt::jwt_config, pwd};
use crate::web::links::change_password_url;
use crate::web::messages::{MessageCode, MessageResponse};
use axum::Json;
use db::ent::User;
use serde::{Deserialize, Serialize};
use smtp::{Smtp, Template};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForgotPasswordRequestBody {
    pub email: String,
    /// The parked authorization request the reset started from, when it began
    /// in an app: the changed password continues there.
    #[serde(default)]
    pub interaction_id: Option<String>,
}

#[utoipa::path(
    post,
    path = "/account/credentials",
    tags = ["Account"],
    summary = "Initiate Password Reset",
    description = "Request a password-reset link for an email address. The response is the same whether or not an account uses the address. While a reset request is pending, a new request resends its link. When the reset starts from an app (`interactionId`), the completed reset returns there.",
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
    check_and_record_email_request(EmailFlow::PasswordReset, &body.email).await?;

    // Resolved before the account lookup, so an address without an account
    // takes the same path whether or not the flow started in an app.
    let continuation =
        crate::iam::oidc::continuation::from_interaction(body.interaction_id.as_deref()).await?;

    // Email only: a nickname or phone number would select a recipient outside
    // the per-recipient throttle's bucket.
    let user = crate::data::db::get_user_by_email(&body.email)
        .await
        .map_err(ErrorResponse::internal)?;

    if let Some(user) = user {
        // Reuse the pending request so a lost or failed email can be resent;
        // the per-recipient throttle bounds the repeats. Issuing the link
        // renews the request, so the two deadlines coincide.
        let sid = pwd::reset::issue_change_password_request(&user.email, continuation)
            .await
            .map_err(ErrorResponse::internal)?;
        let locale = user_locale(&user).unwrap_or_else(smtp::default_locale);
        spawn_reset_email(user, sid, locale);
    }

    // Same response and no delivery wait either way, so neither the body nor
    // the response time reveals whether the address has an account.
    Ok(Json(MessageResponse::new(
        MessageCode::PasswordResetRequested,
    )))
}

fn spawn_reset_email(user: User, sid: String, locale: tools::Locale) {
    std::mem::drop(tokio::task::spawn_blocking(move || {
        if let Err(error) = send_reset_email(&user, sid, locale) {
            tracing::error!(%error, user_id = %user.id, "failed to send password-reset email");
        }
    }));
}

fn send_reset_email(user: &User, sid: String, locale: tools::Locale) -> anyhow::Result<()> {
    let claim = jwt::Claims::default()
        .sub_id(user.id.clone())
        .email(user.email.clone())
        .sid(sid);
    let token = jwt_config().generate_password_reset_token(claim)?;

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    Smtp::new()
        .template(Template::ChangePasswordRequest)
        .locale(locale)
        .to(user.email.clone())
        .name(Some(format_name(&given_name, &family_name)))
        .link(change_password_url(&token))
        .build()
        .and_then(|smtp| smtp.send())?;
    Ok(())
}

#[cfg(all(test, feature = "memory", feature = "sqlite"))]
mod tests {
    use super::{ForgotPasswordRequestBody, post_credentials};
    use crate::data::db::with_test_database;
    use axum::Json;
    use db::ent::{
        CredentialType, Profile, TrustedAuditActor, TrustedAuditContext, TrustedAuditRequest,
    };

    async fn user(email: &str, nickname: &str) {
        let context = TrustedAuditContext::application(
            TrustedAuditActor::anonymous(),
            TrustedAuditRequest::from_http(ulid::Ulid::generate().to_string(), None, None, None),
        );
        crate::data::db::create_user(
            Profile::new(email.to_string(), nickname.to_string()),
            CredentialType::Password,
            "TEST_PASSWORD_HASH",
            None,
            context,
        )
        .await
        .expect("create user");
    }

    async fn request(email: &str, interaction_id: Option<&str>) -> serde_json::Value {
        let response = post_credentials(Json(ForgotPasswordRequestBody {
            email: email.to_string(),
            interaction_id: interaction_id.map(str::to_string),
        }))
        .await
        .expect("password reset requested");
        serde_json::to_value(response.0).expect("response json")
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

    async fn app_client(client_id: &str, initiate_login_uri: &str) {
        let mut client = db::ent::OAuthClient::new(
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
        client.initiate_login_uri = Some(initiate_login_uri.to_string());
        crate::data::db::create_oauth_client(client, admin_context())
            .await
            .expect("create client");
    }

    async fn interaction(client_id: &str) -> String {
        let query = crate::iam::oidc::request::AuthorizationRequest {
            client_id: Some(client_id.to_string()),
            ..Default::default()
        };
        crate::iam::oidc::interactions::park(query)
            .await
            .expect("parked request")
    }

    #[tokio::test]
    async fn the_reset_request_response_is_the_same_for_known_and_unknown_addresses() {
        with_test_database(async {
            user("known@example.com", "known").await;

            let known = request("known@example.com", None).await;
            let unknown = request("unknown@example.com", None).await;
            let with_interaction = request("known@example.com", Some("expired")).await;

            assert_eq!(known, unknown);
            assert_eq!(known, with_interaction);
            assert_eq!(known["code"], "password.reset_requested");
        })
        .await;
    }

    #[tokio::test]
    async fn a_new_link_renews_the_pending_request_it_was_sent_for() {
        with_test_database(async {
            let email = "known@example.com";
            user(email, "known").await;
            app_client("orders", "https://orders.example/login").await;
            request(email, None).await;

            let store = crate::data::store::use_store();
            let key = store::keys::account::password_reset(email);
            let first_deadline = store.expiry(&key).expect("pending request");
            let first_sid = crate::iam::pwd::reset::get_change_password_request(email)
                .await
                .unwrap()
                .unwrap()
                .sid;

            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
            // The link is requested again, this time from an app.
            let interaction = interaction("orders").await;
            request(email, Some(&interaction)).await;

            let renewed_deadline = store.expiry(&key).expect("pending request");
            assert!(
                renewed_deadline > first_deadline + chrono::Duration::seconds(1),
                "{first_deadline} and {renewed_deadline}"
            );
            // The same pending request, so the link already in the mailbox
            // stays valid, now pointing at the newest app.
            let request = crate::iam::pwd::reset::get_change_password_request(email)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(request.sid, first_sid);
            assert_eq!(
                request.continuation,
                Some(crate::iam::oidc::continuation::LoginContinuation::new(
                    "orders", None
                ))
            );
        })
        .await;
    }
}
