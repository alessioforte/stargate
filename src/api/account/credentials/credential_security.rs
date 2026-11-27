//! The same credential flows run against SQLite/memory in the normal suite and
//! PostgreSQL/Redis in the disposable Redis CI fixture.
use super::{expired::maybe_password_expired_response, reset::put_credentials};
use crate::api::account::refresh_token::put_refresh_token;
use crate::err::ErrorCode;
use crate::iam::{auth::jwt::jwt_config, oidc::refresh, pwd, sess::registry};
use axum::body::Body;
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
    create_user_with_password_hash("original-hash").await
}

async fn create_user_with_password_hash(hash: &str) -> User {
    let id = ulid::Ulid::generate().to_string();
    crate::data::db::create_user(
        Profile::new(format!("{id}@example.com"), id),
        CredentialType::Password,
        hash,
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
    let request = axum::http::Request::builder()
        .method("PUT")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            serde_json::json!({ "refresh_token": token }).to_string(),
        ))
        .unwrap();
    let response = put_refresh_token(request).await?;
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
        pwd::hash::check_password(PASSWORD.to_string(), credential.value)
            .await
            .unwrap()
            .valid
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
    let error = crate::api::signup::verification::get_signup(
        http::Request::builder()
            .uri("/signup")
            .header("authorization", format!("Bearer {}", reset.clone()))
            .body(axum::body::Body::empty())
            .unwrap(),
    )
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
    let error = crate::api::signup::verification::get_signup(
        http::Request::builder()
            .uri(format!("/signup?token={verification}"))
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::SignupTokenInvalid);
    let verified = crate::api::signup::verification::get_signup(
        http::Request::builder()
            .uri("/signup")
            .header("authorization", format!("Bearer {}", verification))
            .body(axum::body::Body::empty())
            .unwrap(),
    )
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

async fn assert_parallel_password_limit(distributed: bool) {
    use crate::api::account::login::verify_login_password;

    let password_hash = pwd::hash::hash_password(PASSWORD.to_string())
        .await
        .unwrap();
    let user = create_user_with_password_hash(&password_hash).await;
    let base = if distributed {
        "2001:db8:1"
    } else {
        "2001:db8:2"
    };
    let outcomes = futures_util::future::join_all((0..16).map(|index| {
        let user = &user;
        async move {
            let ip = if distributed {
                format!("{base}::{index:x}")
            } else {
                format!("{base}::1")
            };
            // Both login names resolve to the same account.
            let login = if index % 2 == 0 {
                &user.email
            } else {
                &user.nickname
            };
            verify_login_password(&ip, login, "wrong-password")
                .await
                .unwrap_err()
        }
    }))
    .await;

    let mut checked = 0;
    let mut limited = 0;
    for error in outcomes {
        match error.code {
            ErrorCode::AuthInvalidCredentials => checked += 1,
            ErrorCode::AuthLoginRateLimited => {
                use axum::response::IntoResponse;
                let response = error.into_response();
                let seconds: u64 = response.headers()["retry-after"]
                    .to_str()
                    .unwrap()
                    .parse()
                    .unwrap();
                if distributed {
                    assert!((1..=2).contains(&seconds));
                } else {
                    assert!((31..=300).contains(&seconds));
                }
                limited += 1;
            }
            other => panic!("unexpected login result: {other:?}"),
        }
    }
    assert_eq!((checked, limited), (5, 11));
}

async fn assert_success_refunds_only_its_attempt() {
    use crate::api::account::login::verify_login_password;

    let password_hash = pwd::hash::hash_password(PASSWORD.to_string())
        .await
        .unwrap();
    let user = create_user_with_password_hash(&password_hash).await;
    let ip = "2001:db8:3::1";
    for _ in 0..6 {
        assert!(
            verify_login_password(ip, &user.email, PASSWORD)
                .await
                .is_ok()
        );
    }
    for _ in 0..2 {
        assert_eq!(
            verify_login_password(ip, &user.email, "wrong-password")
                .await
                .unwrap_err()
                .code,
            ErrorCode::AuthInvalidCredentials
        );
    }
    assert!(
        verify_login_password(ip, &user.email, PASSWORD)
            .await
            .is_ok()
    );
    for _ in 0..3 {
        assert_eq!(
            verify_login_password(ip, &user.email, "wrong-password")
                .await
                .unwrap_err()
                .code,
            ErrorCode::AuthInvalidCredentials
        );
    }
    assert_eq!(
        verify_login_password(ip, &user.email, PASSWORD)
            .await
            .unwrap_err()
            .code,
        ErrorCode::AuthLoginRateLimited
    );
}

async fn assert_remaining_address_allowance(ip: &str, remaining: usize) {
    use crate::iam::auth::login_guard::{Admission, LoginThrottle};

    let results = futures_util::future::join_all((0..24).map(|index| async move {
        let throttle = LoginThrottle::for_identifier(ip, &format!("address-test-{index}"));
        matches!(throttle.reserve().await.unwrap(), Admission::Allowed(_))
    }))
    .await;
    assert_eq!(
        results.into_iter().filter(|allowed| *allowed).count(),
        remaining
    );
}

async fn login_guard_suite() {
    assert_parallel_password_limit(false).await;
    assert_parallel_password_limit(true).await;
    assert_success_refunds_only_its_attempt().await;
    // Each address had five failed verifications. Successful and refused
    // requests leave capacity for fifteen more before the source cap of twenty.
    for ip in ["2001:db8:2::1", "2001:db8:3::1"] {
        assert_remaining_address_allowance(ip, 15).await;
    }
}

#[tokio::test]
#[cfg_attr(
    feature = "redis",
    ignore = "requires the disposable PostgreSQL/Redis fixture"
)]
async fn concurrent_password_attempts_respect_login_limits() {
    #[cfg(feature = "memory")]
    crate::data::db::with_test_database(login_guard_suite()).await;
    #[cfg(feature = "redis")]
    {
        crate::data::store::init().await.unwrap();
        crate::data::db::init().await.unwrap();
        login_guard_suite().await;
    }
}

fn login_request(ip: &str, uri: &str, body: Value) -> Request<Body> {
    let mut request = request(uri, body);
    request
        .extensions_mut()
        .insert(crate::srv::test_connection(std::net::SocketAddr::new(
            ip.parse().unwrap(),
            12345,
        )));
    // An untrusted source cannot choose the address that is remembered.
    request
        .headers_mut()
        .insert("x-forwarded-for", "192.0.2.99".parse().unwrap());
    crate::srv::client_ip::prepare(&mut request);
    request
}

/// What the login guard counts for a request from `ip` on a listener with the
/// default IPv6 prefix.
fn client_network(ip: &str) -> String {
    crate::srv::client_ip::Ipv6Prefix::DEFAULT
        .network(ip.parse().unwrap())
        .to_string()
}

async fn assert_remembered(user: &User, client: &str, remembered: bool) {
    let key = store::keys::account::login_known(&user.id);
    let expires_at = crate::data::store::use_store()
        .hget::<i64>(&key, client)
        .await
        .unwrap();
    assert_eq!(expires_at.is_some(), remembered);
    if let Some(expires_at) = expires_at {
        assert!(expires_at > chrono::Utc::now().timestamp());
    }
    assert!(
        !crate::data::store::use_store()
            .hexists(&key, "192.0.2.99")
            .await
            .unwrap()
    );
}

async fn completed_password_login_suite() {
    use crate::api::account::login::{post_login, verify_login_password};
    use crate::iam::oidc::{interactions, request::AuthorizationRequest};
    use crate::iam::otp::{
        DEFAULT_MFA_PURPOSE,
        types::{PendingMfaLogin, StoredOtpChallenge},
        util,
    };

    let hash = pwd::hash::hash_password(PASSWORD.into()).await.unwrap();
    let user = create_user_with_password_hash(&hash).await;
    // Each flow comes from its own /64: recognition is per client network.
    let json_ip = "2001:db8:bbbb:1::1";
    let json_client = client_network(json_ip);
    let credentials = json!({"username": user.email, "password": PASSWORD});

    let wrong = json!({"username": user.email, "password": "wrong"});
    assert_eq!(
        post_login(login_request(json_ip, "/account/login", wrong))
            .await
            .unwrap_err()
            .code,
        ErrorCode::AuthInvalidCredentials
    );
    assert_remembered(&user, &json_client, false).await;
    // Verification alone cannot recognize an address before the rest of the flow.
    verify_login_password(json_ip, &user.email, PASSWORD)
        .await
        .unwrap();
    assert_remembered(&user, &json_client, false).await;
    let invalid_org = json!({"username": user.email, "password": PASSWORD, "orgId": "missing"});
    assert!(
        post_login(login_request(json_ip, "/account/login", invalid_org))
            .await
            .is_err()
    );
    assert_remembered(&user, &json_client, false).await;
    assert_eq!(
        post_login(login_request(json_ip, "/account/login", credentials))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_remembered(&user, &json_client, true).await;

    let form_ip = "2001:db8:bbbb:2::1";
    let interaction = interactions::park(AuthorizationRequest::default())
        .await
        .unwrap();
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("username", &user.email)
        .append_pair("password", PASSWORD)
        .finish();
    let mut form = login_request(form_ip, "/oauth/login", Value::Null);
    *form.body_mut() = Body::from(body);
    form.headers_mut().insert(
        CONTENT_TYPE,
        "application/x-www-form-urlencoded".parse().unwrap(),
    );
    form.headers_mut().insert(
        http::header::COOKIE,
        format!("oauth_interaction={interaction}").parse().unwrap(),
    );
    let response = crate::api::oidc::login::post_login(form).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(
        response.headers()[http::header::LOCATION]
            .to_str()
            .unwrap()
            .ends_with(&interaction)
    );
    assert_remembered(&user, &client_network(form_ip), true).await;

    // Seed a delivered challenge so the real completion endpoint can be tested
    // without SMTP. The password was checked from this address before parking.
    let mfa_ip = "2001:db8:bbbb:3::1";
    let mfa_client = client_network(mfa_ip);
    verify_login_password(&mfa_client, &user.email, PASSWORD)
        .await
        .unwrap();
    assert_remembered(&user, &mfa_client, false).await;
    let now = chrono::Utc::now().timestamp() as u64;
    let issued = otp::issue_email_otp(
        &user.email,
        DEFAULT_MFA_PURPOSE,
        util::email_otp_pepper().unwrap().as_bytes(),
        now,
        util::email_otp_config(),
    )
    .unwrap();
    let challenge_id = issued.record.id_hex();
    let pending = PendingMfaLogin {
        user_id: user.id.clone(),
        method: crate::iam::otp::OtpMethod::Email,
        challenge: StoredOtpChallenge {
            user_id: user.id.clone(),
            method: crate::iam::otp::OtpMethod::Email,
            purpose: DEFAULT_MFA_PURPOSE.into(),
            record: issued.record,
        },
        auth_time: now as usize,
        requested_org_id: None,
        created_at_unix: now,
        expires_at_unix: now + 300,
    };
    crate::data::store::use_store()
        .set(&util::pending_login_key(&challenge_id), &pending, Some(300))
        .await
        .unwrap();
    let verify = crate::api::account::otp::put_login_mfa_challenge;
    assert!(
        verify(
            axum::extract::Path(challenge_id.clone()),
            login_request(
                mfa_ip,
                "/account/login/mfa/challenges/test",
                json!({"code": "wrong"})
            ),
        )
        .await
        .is_err()
    );
    assert_remembered(&user, &mfa_client, false).await;
    assert_eq!(
        verify(
            axum::extract::Path(challenge_id),
            login_request(
                mfa_ip,
                "/account/login/mfa/challenges/test",
                json!({"code": issued.code})
            ),
        )
        .await
        .unwrap()
        .status(),
        StatusCode::OK
    );
    assert_remembered(&user, &mfa_client, true).await;

    // Four distributed failures plus the first wrong password activate the
    // account pause. Remembered addresses can still complete a login.
    for index in 0..4 {
        assert_eq!(
            verify_login_password(
                &format!("2001:db8:cccc::{index}"),
                &user.nickname,
                "wrong-password",
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::AuthInvalidCredentials
        );
    }
    let fresh_ip = "2001:db8:cccc::100";
    assert_eq!(
        verify_login_password(&client_network(fresh_ip), &user.email, PASSWORD)
            .await
            .unwrap_err()
            .code,
        ErrorCode::AuthLoginRateLimited
    );
    // A rotated privacy address in the remembered /64 is the same client.
    for ip in [json_ip, "2001:db8:bbbb:1:a8f3:91ff:fe20:7c41"] {
        let credentials = json!({"username": user.email, "password": PASSWORD});
        assert_eq!(
            post_login(login_request(ip, "/account/login", credentials))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    crate::data::store::use_store()
        .hset(
            &store::keys::account::login_known(&user.id),
            &json_client,
            &0_i64,
            Some(60),
        )
        .await
        .unwrap();
    assert_eq!(
        verify_login_password(&json_client, &user.email, PASSWORD)
            .await
            .unwrap_err()
            .code,
        ErrorCode::AuthLoginRateLimited
    );

    forgotten_after_credential_change(&user).await;
}

/// A password change revokes every session through the same call used by both
/// reset and expired-password completion; revoking every session directly must
/// have the same effect on recognition.
async fn forgotten_after_credential_change(user: &User) {
    use crate::api::account::login::{post_login, verify_login_password};
    use crate::iam::auth::login_guard;

    let ip = "2001:db8:dddd::1";
    let client = client_network(ip);
    login_guard::record_success(&user.id, &client).await;
    login_guard::seed_account_backoff(&user.id).await;
    assert!(
        verify_login_password(&client, &user.email, PASSWORD)
            .await
            .is_ok()
    );

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
    let _ = put_credentials(request(
        "/account/credentials",
        json!({"token": token, "password": PASSWORD}),
    ))
    .await
    .unwrap();
    assert_remembered(user, &client, false).await;
    assert_eq!(
        verify_login_password(&client, &user.email, PASSWORD)
            .await
            .unwrap_err()
            .code,
        ErrorCode::AuthLoginRateLimited
    );

    // A login completed after the reset registers the address again.
    crate::data::store::use_store()
        .delete(&store::keys::account::login_user(&user.id, None))
        .await
        .unwrap();
    assert_eq!(
        post_login(login_request(
            ip,
            "/account/login",
            json!({"username": user.email.clone(), "password": PASSWORD}),
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::OK
    );
    assert_remembered(user, &client, true).await;

    login_guard::seed_account_backoff(&user.id).await;
    assert!(
        verify_login_password(&client, &user.email, PASSWORD)
            .await
            .is_ok()
    );
    assert!(registry::revoke_all_sessions(&user.id).await.unwrap() > 0);
    assert_remembered(user, &client, false).await;
    assert_eq!(
        verify_login_password(&client, &user.email, PASSWORD)
            .await
            .unwrap_err()
            .code,
        ErrorCode::AuthLoginRateLimited
    );
}

#[tokio::test]
#[cfg_attr(
    feature = "redis",
    ignore = "requires the disposable PostgreSQL/Redis fixture"
)]
async fn completed_password_logins_remember_server_addresses() {
    const CHILD: &str = "STARGATE_TEST_REMEMBER_LOGIN_ADDRESSES";
    if std::env::var_os(CHILD).is_none() {
        let output = tokio::task::spawn_blocking(|| {
            crate::conf::child_test::run(
                "api::account::credentials::credential_security::completed_password_logins_remember_server_addresses",
                |command| {
                    command
                        .env(CHILD, "1")
                        .env("MFA_MODE", "optional")
                        .env("EMAIL_OTP_PEPPER", "synthetic-login-test-pepper");
                },
            )
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    #[cfg(feature = "memory")]
    crate::data::db::with_test_database(completed_password_login_suite()).await;
    #[cfg(feature = "redis")]
    {
        crate::data::store::init().await.unwrap();
        crate::data::db::init().await.unwrap();
        completed_password_login_suite().await;
    }
}
