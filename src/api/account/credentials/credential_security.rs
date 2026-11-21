//! The same credential flows run against SQLite/memory in the normal suite and
//! PostgreSQL/Redis in the disposable Redis CI fixture.
use super::{expired::maybe_password_expired_response, reset::put_credentials};
use crate::api::account::{RefreshTokenRequestBody, refresh_token::put_refresh_token};
use crate::err::ErrorCode;
use crate::iam::{auth::jwt::jwt_config, oidc::refresh, pwd, sess::registry};
use axum::{Json, body::Body};
use db::ent::{
    CredentialType, OrganizationMembershipMutation, Profile, TrustedAuditActor,
    TrustedAuditContext, TrustedAuditRequest, User,
};
use http::{Request, StatusCode, header::CONTENT_TYPE};
use serde_json::{Value, json};
use store::Store;

const PASSWORD: &str = "Str0ng-Replacement!42";

fn request(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn admin_context() -> TrustedAuditContext {
    TrustedAuditContext::admin_control_plane(
        db::ent::TrustedAdminActor::admin("01JZ000000000000000000000A"),
        TrustedAuditRequest::from_http(ulid::Ulid::generate().to_string(), None, None, None),
    )
}

async fn create_user() -> User {
    let id = ulid::Ulid::generate().to_string();
    crate::data::db::create_user(
        Profile::new(format!("{id}@example.com"), id),
        CredentialType::Password,
        "original-hash",
        None,
        TrustedAuditContext::application(
            TrustedAuditActor::anonymous(),
            TrustedAuditRequest::from_http(ulid::Ulid::generate().to_string(), None, None, None),
        ),
    )
    .await
    .unwrap()
}

async fn session(user: &User) -> crate::iam::sess::issuance::IssuedSession {
    crate::iam::sess::issuance::start_user_session(
        user.clone(),
        chrono::Utc::now().timestamp() as usize,
        None,
        vec!["pwd".to_string()],
    )
    .await
    .unwrap()
}

async fn oauth_family(user: &User, sid: &str) -> String {
    refresh::issue(
        "credential-test".to_string(),
        user.id.clone(),
        Some(sid.to_string()),
        "openid offline_access".to_string(),
        None,
        chrono::Utc::now(),
        None,
        oidc::authentication::Authentication {
            amr: vec!["pwd".to_string()],
            acr: oidc::authentication::ACR_SINGLE_FACTOR.to_string(),
        },
        3600,
    )
    .await
    .unwrap()
}

async fn refresh_native(token: &str) -> Result<Value, crate::err::ErrorResponse> {
    let response = put_refresh_token(Json(RefreshTokenRequestBody {
        refresh_token: token.to_string(),
    }))
    .await?;
    let bytes = axum::body::to_bytes(response.into_body(), 32 * 1024)
        .await
        .unwrap();
    Ok(serde_json::from_slice(&bytes).unwrap())
}

#[cfg(feature = "redis")]
async fn backchannel_receiver(
    user: &User,
    sid: &str,
) -> (
    tokio::sync::mpsc::Receiver<String>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://{}/logout", listener.local_addr().unwrap());
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    let router = axum::Router::new().route(
        "/logout",
        axum::routing::post(
            move |axum::Form(form): axum::Form<std::collections::HashMap<String, String>>| {
                let sender = sender.clone();
                async move {
                    sender.send(form["logout_token"].clone()).await.unwrap();
                    StatusCode::OK
                }
            },
        ),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client_id = ulid::Ulid::generate().to_string();
    let mut client = db::ent::OAuthClient::new(
        client_id.clone(),
        None,
        "Credential logout test".to_string(),
        None,
        "none".to_string(),
        vec!["authorization_code".to_string()],
        vec!["code".to_string()],
        vec!["https://example.com/callback".to_string()],
        vec!["openid".to_string()],
        Vec::new(),
        json!({}),
    );
    client.backchannel_logout_uri = Some(uri);
    crate::data::db::create_oauth_client(client, admin_context())
        .await
        .unwrap();
    assert!(
        registry::register_session_client(sid, &user.id, &client_id)
            .await
            .unwrap()
    );
    (receiver, server)
}

async fn reset_revokes_every_session(expired: bool) {
    let user = create_user().await;
    let first = session(&user).await;
    let second = session(&user).await;
    let unaffected_user = create_user().await;
    let unaffected = session(&unaffected_user).await;
    let sid = jwt_config()
        .validate_session_access_token(&first.access_token)
        .unwrap()
        .sid
        .unwrap();
    let family = oauth_family(&user, &sid).await;
    #[cfg(feature = "redis")]
    let (mut logout, server) = backchannel_receiver(&user, &sid).await;
    let second_sid = jwt_config()
        .validate_session_access_token(&second.access_token)
        .unwrap()
        .sid
        .unwrap();
    let second_family = oauth_family(&user, &second_sid).await;

    let reset_token = if expired {
        let organization = crate::data::db::create_organization(
            &ulid::Ulid::generate().to_string(),
            None,
            Some(&json!({"password_policy": {"max_age_days": 1}})),
            admin_context(),
        )
        .await
        .unwrap();
        crate::data::db::add_user_to_organization(
            &user.id,
            &organization.id,
            "member",
            OrganizationMembershipMutation::Create,
            admin_context(),
        )
        .await
        .unwrap();
        let mut credential = crate::data::db::get_credential(&user.id, CredentialType::Password)
            .await
            .unwrap()
            .unwrap();
        credential.updated_at = chrono::Utc::now() - chrono::Duration::days(2);
        let response = maybe_password_expired_response(&user, Some(&credential))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let bytes = axum::body::to_bytes(response.into_body(), 32 * 1024)
            .await
            .unwrap();
        serde_json::from_slice::<Value>(&bytes).unwrap()["resetToken"]
            .as_str()
            .unwrap()
            .to_string()
    } else {
        let sid = pwd::reset::issue_change_password_request(&user.email, None)
            .await
            .unwrap();
        jwt_config()
            .generate_password_reset_token(
                jwt::Claims::default()
                    .sub_id(user.id.clone())
                    .email(user.email.clone())
                    .sid(sid),
            )
            .unwrap()
    };
    let body = json!({"token": reset_token, "password": PASSWORD});
    let _ = put_credentials(request("/account/credentials", body.clone()))
        .await
        .unwrap();
    assert_eq!(
        put_credentials(request("/account/credentials", body))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PasswordResetRequestInvalid
    );
    for tokens in [&first, &second] {
        assert_eq!(
            registry::authenticated_session(Some(tokens.access_token.clone()))
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::AuthTokenInvalid
        );
        assert_eq!(
            refresh_native(&tokens.refresh_token)
                .await
                .unwrap_err()
                .status,
            StatusCode::UNAUTHORIZED
        );
    }
    for family in [&family, &second_family] {
        assert!(
            refresh::rotate(family, "credential-test")
                .await
                .unwrap()
                .is_none()
        );
        let (id, _) = oidc::refresh::parse_refresh_token(family).unwrap();
        assert!(
            !crate::data::store::use_store()
                .exists(&oidc::refresh::family_key(id))
                .await
                .unwrap()
        );
    }
    assert!(
        pwd::reset::get_change_password_request(&user.email)
            .await
            .unwrap()
            .is_none()
    );
    #[cfg(feature = "redis")]
    {
        let notification = tokio::time::timeout(std::time::Duration::from_secs(5), logout.recv())
            .await
            .unwrap()
            .unwrap();
        let claims = jwt_config().validate_token(&notification).unwrap();
        assert_eq!(claims.sid.as_deref(), Some(sid.as_str()));
        assert_eq!(claims.sub, user.id);
        server.abort();
    }
    let credential = crate::data::db::get_credential(&user.id, CredentialType::Password)
        .await
        .unwrap()
        .unwrap();
    assert!(
        pwd::hash::verify_password(PASSWORD.to_string(), credential.value)
            .await
            .unwrap()
    );
    assert!(
        registry::authenticated_session(Some(unaffected.access_token))
            .await
            .is_ok()
    );
    registry::revoke_all_sessions(&unaffected_user.id)
        .await
        .unwrap();
}

async fn refresh_replay_ends_the_session() {
    let user = create_user().await;
    let tokens = session(&user).await;
    let next = refresh_native(&tokens.refresh_token).await.unwrap();
    assert_eq!(
        refresh_native(&tokens.refresh_token)
            .await
            .unwrap_err()
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        refresh_native(next["refreshToken"].as_str().unwrap())
            .await
            .unwrap_err()
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert!(
        registry::authenticated_session(Some(tokens.access_token))
            .await
            .is_err()
    );
}

async fn a_reset_request_is_consumed_once() {
    let user = create_user().await;
    let sid = pwd::reset::issue_change_password_request(&user.email, None)
        .await
        .unwrap();
    let token = jwt_config()
        .generate_password_reset_token(
            jwt::Claims::default()
                .sub_id(user.id.clone())
                .email(user.email.clone())
                .sid(sid),
        )
        .unwrap();
    let invalid = put_credentials(request(
        "/account/credentials",
        json!({"token": token, "password": "short"}),
    ))
    .await
    .unwrap_err();
    assert_eq!(invalid.code, ErrorCode::PasswordPolicyViolation);
    assert!(
        pwd::reset::get_change_password_request(&user.email)
            .await
            .unwrap()
            .is_some()
    );
    let body = json!({"token": token, "password": PASSWORD});
    let (left, right) = tokio::join!(
        put_credentials(request("/account/credentials", body.clone())),
        put_credentials(request("/account/credentials", body)),
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let error = left.err().or(right.err()).unwrap();
    assert_eq!(error.code, ErrorCode::PasswordResetRequestInvalid);
}

async fn tokens_are_bound_to_their_flow() {
    let user = create_user().await;
    let sid = pwd::reset::issue_change_password_request(&user.email, None)
        .await
        .unwrap();
    let claims = jwt::Claims::default()
        .sub_id(user.id.clone())
        .email(user.email.clone())
        .sid(sid);
    for token in [
        jwt_config().generate_token(&claims).unwrap(),
        jwt_config().generate_signup_token(claims.clone()).unwrap(),
    ] {
        let error = put_credentials(request(
            "/account/credentials",
            json!({"token": token, "password": PASSWORD}),
        ))
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::PasswordResetTokenInvalid);
    }
    let reset = jwt_config().generate_password_reset_token(claims).unwrap();
    let error = crate::api::signup::verification::get_signup(axum::extract::Query(
        crate::api::signup::SignupVerificationParams {
            token: reset.clone(),
        },
    ))
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::SignupTokenInvalid);
    let error = crate::api::signup::complete::put_signup(request("/signup", json!({
        "token": reset, "password": PASSWORD, "givenName": "", "familyName": "", "nickname": "", "phoneNumber": null,
    }))).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::SignupTokenInvalid);
}

async fn uniqueness_conflicts_are_conflicts() {
    use crate::iam::user::{email_verification, signup};
    let phone = format!("+{}", &ulid::Ulid::generate().to_string()[..15]);
    // Phone uniqueness has no preflight check: this exercises the actual insert
    // error path used when email/nickname preflight checks lose a signup race.
    let mut first = create_user().await;
    first.phone_number = Some(phone.clone());
    crate::data::db::update_user(first, admin_context())
        .await
        .unwrap();
    let email = format!("{}@example.com", ulid::Ulid::generate());
    let pending = signup::PendingSignupProfile {
        origin: signup::SignupOrigin::Invitation,
        ..signup::PendingSignupProfile::self_service(email.clone(), tools::Locale::En)
    };
    let sid = email_verification::create_email_verification_request(&pending)
        .await
        .unwrap();
    let verification = jwt_config()
        .generate_signup_verification_token(
            jwt::Claims::default()
                .subject("signup_request".to_string())
                .sid(sid)
                .email(email),
        )
        .unwrap();
    let verified = crate::api::signup::verification::get_signup(axum::extract::Query(
        crate::api::signup::SignupVerificationParams {
            token: verification,
        },
    ))
    .await
    .unwrap()
    .0;
    let body = json!({"token": verified.token, "password": PASSWORD, "givenName": "", "familyName": "", "nickname": "", "phoneNumber": phone});
    let error = crate::api::signup::complete::put_signup(request("/signup", body.clone()))
        .await
        .unwrap_err();
    assert_eq!(error.status, StatusCode::CONFLICT);
    assert_eq!(error.code, ErrorCode::SignupUserAlreadyExists);
    // A failed insert did not consume the signup or partially create the user.
    assert!(
        crate::data::db::get_user_by_email(&pending.email)
            .await
            .unwrap()
            .is_none()
    );
    let mut retry = body;
    retry["phoneNumber"] = Value::Null;
    let _ = crate::api::signup::complete::put_signup(request("/signup", retry))
        .await
        .unwrap();
}

async fn suite() {
    crate::srv::tls::install_crypto_provider();
    reset_revokes_every_session(false).await;
    reset_revokes_every_session(true).await;
    refresh_replay_ends_the_session().await;
    tokens_are_bound_to_their_flow().await;
    a_reset_request_is_consumed_once().await;
    uniqueness_conflicts_are_conflicts().await;
}

#[tokio::test]
#[cfg_attr(
    feature = "redis",
    ignore = "requires the disposable PostgreSQL/Redis fixture"
)]
async fn credential_flows() {
    #[cfg(feature = "memory")]
    crate::data::db::with_test_database(suite()).await;
    #[cfg(feature = "redis")]
    {
        crate::data::store::init().await.unwrap();
        crate::data::db::init().await.unwrap();
        suite().await;
    }
}
