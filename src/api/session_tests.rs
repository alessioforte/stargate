use crate::web::request::RequestExt;
mod concurrency;
#[cfg(feature = "redis")]
mod delayed_publication;
mod delegated;

use crate::data::{db, store::use_store};
use crate::iam::{
    auth::{jwt::jwt_config, subject::Subject, verification::verify_bearer},
    oidc::refresh,
    sess::{issuance, registry},
};
use ::db::ent::{
    CredentialType, OAuthClient, OrganizationMembershipMutation, Profile, TrustedAdminActor,
    TrustedAuditContext, TrustedAuditRequest, User,
};
use axum::{Extension, Router, body::Body};
use http::{Request, StatusCode, header};
use serde_json::{Value, json};
use std::sync::Arc;
use store::Store;
use tower::ServiceExt;

const CLIENT_ID: &str = "session-tests";
const CLIENT_SECRET: &str = "fixture-client-secret";
const AUDIENCE: &str = "gateway";

fn context() -> TrustedAuditContext {
    TrustedAuditContext::admin_control_plane(
        TrustedAdminActor::admin("01JZ000000000000000000000A"),
        TrustedAuditRequest::from_http(ulid::Ulid::generate().to_string(), None, None, None),
    )
}

async fn create_user() -> User {
    let name = ulid::Ulid::generate().to_string();
    db::create_user(
        Profile::new(format!("{name}@example.test"), name),
        CredentialType::Password,
        "fixture-password-hash",
        None,
        context(),
    )
    .await
    .unwrap()
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn send(router: &Router, req: Request<Body>, expected: StatusCode) -> Value {
    let uri = req.uri().clone();
    let response = router.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 32 * 1024)
        .await
        .unwrap();
    assert_eq!(
        status,
        expected,
        "{uri}: {}",
        String::from_utf8_lossy(&body)
    );
    serde_json::from_slice(&body).unwrap()
}

async fn json_request(
    router: &Router,
    method: &str,
    path: &str,
    credential: (&str, &str),
    body: Value,
) -> Value {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header(credential.0, credential.1)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(router, req, StatusCode::OK).await
}

async fn oauth_form(router: &Router, path: &str, fields: &[(&str, &str)]) -> Value {
    send(router, oauth_request(path, fields), StatusCode::OK).await
}

fn oauth_request(path: &str, fields: &[(&str, &str)]) -> Request<Body> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", CLIENT_ID)
        .append_pair("client_secret", CLIENT_SECRET)
        .extend_pairs(fields.iter().copied())
        .finish();
    Request::post(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap()
}

async fn userinfo(router: &Router, token: &str, status: StatusCode) {
    let req = Request::get("/oauth/userinfo")
        .header(header::AUTHORIZATION, bearer(token))
        .body(Body::empty())
        .unwrap();
    send(router, req, status).await;
}

fn delegated_token(user: &User, sid: Option<&str>) -> String {
    jwt_config()
        .generate_oauth_access_token(jwt::Claims {
            sub: user.id.clone(),
            sub_id: Some(user.id.clone()),
            sid: sid.map(str::to_string),
            azp: Some(CLIENT_ID.into()),
            aud: Some(AUDIENCE.into()),
            auth_time: Some(1),
            scope: Some("openid".into()),
            ..Default::default()
        })
        .unwrap()
}

async fn start_session(user: &User, org_id: Option<&str>) -> (String, String, String, String) {
    let native = issuance::start_user_session(
        user.clone(),
        chrono::Utc::now().timestamp() as usize,
        org_id,
        vec!["pwd".into()],
    )
    .await
    .unwrap();
    let sid = jwt_config()
        .validate_session_access_token(&native.access_token)
        .unwrap()
        .sid
        .unwrap();
    assert!(
        registry::register_session_client(&sid, &user.id, CLIENT_ID)
            .await
            .unwrap()
    );
    let oauth = delegated_token(user, Some(&sid));
    (sid, native.access_token, native.refresh_token, oauth)
}

fn refresh_jti(refresh_token: &str) -> String {
    jwt_config()
        .validate_session_refresh_token(refresh_token)
        .unwrap()
        .jti
        .unwrap()
}

async fn subject(token: &str) -> Subject {
    let req = Request::builder()
        .header(header::AUTHORIZATION, bearer(token))
        .body(Body::empty())
        .unwrap();
    verify_bearer(req.get_token())
        .await
        .expect("security state available")
        .expect("valid session token")
        .subject()
        .expect("user subject")
        .clone()
}

async fn assert_subject(tokens: [&str; 2], attrs: &Value, organization: Option<(&str, &str)>) {
    for token in tokens {
        let subject = subject(token).await;
        assert_eq!(&subject.attrs, attrs);
        assert_eq!(subject.org_id.as_deref(), organization.map(|(id, _)| id));
        assert_eq!(
            subject.org_role.as_deref(),
            organization.map(|(_, role)| role)
        );
    }
}

async fn revise_session(sid: &str, revise: impl FnOnce(&mut registry::SessionRecord)) {
    let mut session = registry::get_session(sid).await.unwrap().unwrap();
    revise(&mut session);
    use_store()
        .set(&store::keys::session::record(sid), &session, Some(60))
        .await
        .unwrap();
}

async fn verify_delegated_session_checks(router: &Router, user: &User, sid: &str, valid: &str) {
    userinfo(router, valid, StatusCode::OK).await;
    assert_eq!(
        oauth_form(router, "/oauth/introspect", &[("token", valid)]).await["active"],
        true
    );

    let other_user = create_user().await;
    let (expired_sid, _, _, expired_token) = start_session(&other_user, None).await;
    revise_session(&expired_sid, |session| {
        session.expires_at_unix = 1;
        session.client_ids.clear();
    })
    .await;

    for token in [
        delegated_token(user, None),
        delegated_token(&other_user, Some(sid)),
        expired_token,
    ] {
        userinfo(router, &token, StatusCode::UNAUTHORIZED).await;
        assert_eq!(
            oauth_form(router, "/oauth/introspect", &[("token", &token)]).await,
            json!({"active": false})
        );
    }

    registry::revoke_session(&expired_sid).await.unwrap();

    let machine = oauth_form(
        router,
        "/oauth/token",
        &[("grant_type", "client_credentials"), ("scope", "openid")],
    )
    .await;
    let machine_token = machine["access_token"].as_str().unwrap();
    userinfo(router, machine_token, StatusCode::UNAUTHORIZED).await;
    assert_eq!(
        oauth_form(router, "/oauth/introspect", &[("token", machine_token)]).await["active"],
        true
    );
}

async fn refresh_family(user: &User, sid: &str) -> String {
    refresh::issue(
        CLIENT_ID.into(),
        user.id.clone(),
        Some(sid.to_owned()),
        "openid offline_access".into(),
        Some(AUDIENCE.into()),
        chrono::Utc::now(),
        None,
        oidc::authentication::Authentication {
            amr: vec!["pwd".into()],
            acr: oidc::authentication::ACR_SINGLE_FACTOR.into(),
        },
        3600,
    )
    .await
    .unwrap()
}

async fn suite() {
    db::create_oauth_client(
        OAuthClient::new(
            CLIENT_ID.into(),
            Some(pw::hash_api_key(CLIENT_SECRET)),
            "Session test".into(),
            None,
            "client_secret_post".into(),
            vec!["client_credentials".into(), "refresh_token".into()],
            vec![],
            vec![],
            vec!["openid".into(), "offline_access".into()],
            vec![AUDIENCE.into()],
            json!({"first_party": true}),
        ),
        context(),
    )
    .await
    .unwrap();
    let admin_key = pw::generate_api_key();
    db::create_admin_key(
        &pw::hash_api_key(&admin_key),
        None,
        [
            "users:update_attrs",
            "memberships:create",
            "memberships:update",
            "memberships:delete",
            "sessions:read",
            "overview:read",
        ]
        .map(str::to_string)
        .to_vec(),
        context(),
    )
    .await
    .unwrap();
    let gateway = crate::gate::Gate::new(
        Arc::new(use_store().clone()),
        crate::gate::prepare_config(gate::cfg::Config::default()).unwrap(),
        gate::PolicySnapshot::default(),
        None,
    )
    .unwrap();
    let router = super::router().layer(Extension(Arc::new(gateway)));

    let response = send(
        &router,
        Request::get("/admin/me")
            .header("x-api-key", &admin_key)
            .body(Body::empty())
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert!(response["adminKeyId"].is_string());

    let user = create_user().await;
    let (sid, native, native_refresh, oauth) = start_session(&user, None).await;
    verify_delegated_session_checks(&router, &user, &sid, &oauth).await;

    let org = db::create_organization("Session test", None, None, context())
        .await
        .unwrap();
    db::add_user_to_organization(
        &user.id,
        &org.id,
        "admin",
        OrganizationMembershipMutation::Create,
        context(),
    )
    .await
    .unwrap();
    let (org_sid, org_native, _, org_oauth) = start_session(&user, Some(&org.id)).await;

    let admin = ("x-api-key", admin_key.as_str());
    let attrs_path = format!("/admin/users/{}/attrs", user.id);
    for (method, attrs, expected) in [
        (
            "PUT",
            json!({"tier": "pro", "grants": ["read", "write"]}),
            json!({"tier": "pro", "grants": ["read", "write"]}),
        ),
        (
            "PATCH",
            json!({"grants": ["read"]}),
            json!({"tier": "pro", "grants": ["read"]}),
        ),
        ("PUT", json!({}), json!({})),
    ] {
        json_request(&router, method, &attrs_path, admin, json!({"attrs": attrs})).await;
        assert_subject([&native, &oauth], &expected, None).await;
        assert_subject(
            [&org_native, &org_oauth],
            &expected,
            Some((&org.id, "admin")),
        )
        .await;
    }

    let membership = format!("/admin/users/{}/organizations/{}", user.id, org.id);
    json_request(
        &router,
        "PUT",
        &membership,
        admin,
        json!({"role": "member"}),
    )
    .await;
    assert_subject([&native, &oauth], &json!({}), None).await;
    assert_subject(
        [&org_native, &org_oauth],
        &json!({}),
        Some((&org.id, "member")),
    )
    .await;

    let auth = bearer(&oauth);
    json_request(
        &router,
        "PUT",
        "/account/profile",
        ("authorization", &auth),
        json!({"locale": "it"}),
    )
    .await;
    let expected = json!({"locale": "it"});
    assert_subject([&native, &oauth], &expected, None).await;
    assert_subject(
        [&org_native, &org_oauth],
        &expected,
        Some((&org.id, "member")),
    )
    .await;

    let refresh_token = refresh_family(&user, &sid).await;

    // Represent a subject publication after a refresh has taken its SQL
    // snapshot. Any refresh that writes its own snapshot will erase this value.
    let key = store::keys::session::subject(&sid);
    let current = use_store()
        .get::<crate::iam::sess::subjects::PublishedSubject>(&key)
        .await
        .unwrap()
        .unwrap();
    let mut published = current.clone();
    published.revision += 1;
    published.subject.attrs = json!({"locale": "it", "publication": "new"});
    assert!(
        use_store()
            .commit(
                &store::Transaction::new()
                    .swap(&key, &current, &published)
                    .unwrap()
            )
            .await
            .unwrap()
    );

    let refreshed = json_request(
        &router,
        "PUT",
        "/account/refresh-token",
        ("authorization", &bearer(&native)),
        json!({"refresh_token": native_refresh}),
    )
    .await;
    let refreshed_oauth = oauth_form(
        &router,
        "/oauth/token",
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh_token),
        ],
    )
    .await;
    assert_subject(
        [
            refreshed["accessToken"].as_str().unwrap(),
            refreshed_oauth["access_token"].as_str().unwrap(),
        ],
        &published.subject.attrs,
        None,
    )
    .await;

    // Edge tests use a task-local database. Detach the clients before the
    // revocations, which perform back-channel delivery in a separate task.
    revise_session(&org_sid, |session| session.client_ids.clear()).await;
    json_request(&router, "DELETE", &membership, admin, Value::Null).await;
    userinfo(&router, &org_oauth, StatusCode::UNAUTHORIZED).await;
    assert_eq!(
        oauth_form(&router, "/oauth/introspect", &[("token", &org_oauth)]).await,
        json!({"active": false})
    );
    assert!(
        crate::iam::sess::subjects::read(&org_sid)
            .await
            .unwrap()
            .is_none()
    );
    userinfo(&router, &oauth, StatusCode::OK).await;
    assert_subject([&native, &oauth], &expected, None).await;
    revise_session(&sid, |session| session.client_ids.clear()).await;
    registry::revoke_session(&sid).await.unwrap();
    delegated::verify_subjects(&router).await;
    concurrency::run(&router, admin).await;
    session_views(&router, admin).await;
}

/// The admin listing pages through a user's sessions, which the overview does
/// not count.
async fn session_views(router: &Router, admin: (&str, &str)) {
    let get = |path: String| {
        Request::get(path)
            .header(admin.0, admin.1)
            .body(Body::empty())
            .unwrap()
    };
    let user = create_user().await;
    let mut sids = vec![
        start_session(&user, None).await.0,
        start_session(&user, None).await.0,
    ];
    sids.sort_unstable_by(|left, right| right.cmp(left));

    let path = format!("/admin/sessions?userId={}&limit=1", user.id);
    let mut listed = Vec::new();
    let mut page = send(router, get(path.clone()), StatusCode::OK).await;
    loop {
        let data = page["data"].as_array().unwrap();
        assert!(data.len() <= 1);
        listed.extend(data.iter().map(|session| session["id"].clone()));
        let Some(cursor) = page["nextCursor"].as_str() else {
            break;
        };
        page = send(
            router,
            get(format!("{path}&cursor={cursor}")),
            StatusCode::OK,
        )
        .await;
    }
    assert_eq!(listed, sids);
    send(
        router,
        get(format!("{path}&cursor=malformed")),
        StatusCode::BAD_REQUEST,
    )
    .await;

    let overview = send(router, get("/admin/overview".into()), StatusCode::OK).await;
    assert!(overview["resources"]["users"]["total"].as_i64().unwrap() >= 1);
    assert_eq!(overview.get("sessions"), None);

    registry::revoke_all_sessions(&user.id).await.unwrap();
}

#[tokio::test]
#[cfg_attr(
    feature = "redis",
    ignore = "requires the disposable PostgreSQL/Redis fixture"
)]
async fn session_flows() {
    crate::srv::tls::install_crypto_provider();
    #[cfg(feature = "memory")]
    db::with_test_database(suite()).await;
    #[cfg(feature = "redis")]
    {
        crate::data::store::init().await.unwrap();
        db::init().await.unwrap();
        suite().await;
    }
}
