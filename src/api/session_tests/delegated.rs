use super::*;
use crate::iam::sess::subjects::PublishedSubject;
use ::db::DbStore;

async fn verify_endpoints(router: &Router, user: &User, sid: &str, oauth: &str, active: bool) {
    let status = if active {
        StatusCode::OK
    } else {
        StatusCode::UNAUTHORIZED
    };
    let admin = Request::get("/admin/me")
        .header(header::AUTHORIZATION, bearer(oauth))
        .body(Body::empty())
        .unwrap();
    let body = send(router, admin, status).await;
    if active {
        assert_eq!(body["user"]["id"], user.id);
    }
    userinfo(router, oauth, status).await;
    assert_eq!(
        oauth_form(router, "/oauth/introspect", &[("token", oauth)]).await["active"],
        active
    );

    let request = Request::get("/")
        .header(header::AUTHORIZATION, bearer(oauth))
        .body(Body::empty())
        .unwrap();
    assert_eq!(verify_bearer(&request).await.is_some(), active);

    let token = refresh_family(user, sid).await;
    let request = oauth_request(
        "/oauth/token",
        &[("grant_type", "refresh_token"), ("refresh_token", &token)],
    );
    if active {
        assert!(send(router, request, StatusCode::OK).await["access_token"].is_string());
    } else {
        assert_eq!(
            send(router, request, StatusCode::BAD_REQUEST).await["error"],
            "invalid_grant"
        );
    }
}

pub(super) async fn verify_subjects(router: &Router) {
    let name = ulid::Ulid::generate().to_string();
    let user = db::service()
        .create_super_admin_user(
            Profile::new(format!("{name}@example.test"), name),
            CredentialType::Password,
            "fixture-password-hash",
            TrustedAuditContext::system_control_plane(),
        )
        .await
        .unwrap();
    let (sid, _, _, oauth) = start_session(&user, None).await;
    let key = store::keys::session::subject(&sid);
    let store = use_store();
    let current = store.get::<PublishedSubject>(&key).await.unwrap().unwrap();

    // Keep the directory and JWT intact and vary just the subject under the sid.
    store.set(&key, &current.subject, Some(60)).await.unwrap();
    verify_endpoints(router, &user, &sid, &oauth, false).await;
    let other = PublishedSubject::from(Subject::from(create_user().await));
    store.set(&key, &other, Some(60)).await.unwrap();
    verify_endpoints(router, &user, &sid, &oauth, false).await;
    store.set(&key, &current, Some(60)).await.unwrap();
    verify_endpoints(router, &user, &sid, &oauth, true).await;

    revise_session(&sid, |record| record.client_ids.clear()).await;
    registry::revoke_session(&sid).await.unwrap();
}
