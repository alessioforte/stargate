use super::*;
use crate::iam::sess::subjects::PublishedSubject;
use crate::web::request::RequestExt;
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
    assert_eq!(
        verify_bearer(request.get_token()).await.unwrap().is_some(),
        active
    );

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

async fn verify_credential_sources(router: &Router, native: &str, oauth: &str) {
    for token in [native, oauth] {
        let response = send(
            router,
            Request::get("/admin/me")
                .header(header::AUTHORIZATION, bearer(token))
                .body(Body::empty())
                .unwrap(),
            StatusCode::OK,
        )
        .await;
        assert!(response["user"]["id"].is_string());
        let response = send(
            router,
            Request::get("/admin/me")
                .header(header::COOKIE, format!("jwt={token}"))
                .body(Body::empty())
                .unwrap(),
            StatusCode::UNAUTHORIZED,
        )
        .await;
        assert_eq!(response["code"], "auth.token_missing");
        for key in crate::web::request::TOKEN_QUERY_KEYS {
            for path in ["/admin/me", "/account/profile"] {
                let response = send(
                    router,
                    Request::get(format!("{path}?{key}={token}"))
                        .body(Body::empty())
                        .unwrap(),
                    StatusCode::UNAUTHORIZED,
                )
                .await;
                assert_eq!(response["code"], "auth.token_missing");
            }
        }
    }
    let response = send(
        router,
        Request::get("/account/profile")
            .header(header::COOKIE, format!("jwt={native}"))
            .body(Body::empty())
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert!(response["id"].is_string());
    for site in ["same-site", "cross-site"] {
        let response = send(
            router,
            Request::get("/account/profile")
                .header(header::COOKIE, format!("jwt={native}"))
                .header("sec-fetch-site", site)
                .body(Body::empty())
                .unwrap(),
            StatusCode::FORBIDDEN,
        )
        .await;
        assert_eq!(response["code"], "auth.untrusted_origin");
    }
    for origin in ["https://evil.example.com", "null"] {
        let response = send(
            router,
            Request::put("/account/profile")
                .header(header::COOKIE, format!("jwt={native}"))
                .header(header::ORIGIN, origin)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"locale":"it"}"#))
                .unwrap(),
            StatusCode::FORBIDDEN,
        )
        .await;
        assert_eq!(response["code"], "auth.untrusted_origin");
    }

    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("token", oauth)
        .finish();
    let token_request = |path| {
        Request::post(path)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::ORIGIN, "https://client.example.com")
    };
    for path in ["/oauth/introspect", "/oauth/revoke"] {
        let response = send(
            router,
            token_request(path)
                .header(header::COOKIE, format!("jwt={native}"))
                .body(Body::from(form.clone()))
                .unwrap(),
            StatusCode::FORBIDDEN,
        )
        .await;
        assert_eq!(response["error"], "insufficient_scope");
    }
    let response = send(
        router,
        token_request("/oauth/introspect")
            .header(header::AUTHORIZATION, bearer(native))
            .body(Body::from(form.clone()))
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(response["active"], true);
    let response = router
        .clone()
        .oneshot(
            token_request("/oauth/revoke")
                .header(header::AUTHORIZATION, bearer(native))
                .body(Body::from(form.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = send(
        router,
        token_request("/oauth/introspect")
            .header(header::AUTHORIZATION, bearer(native))
            .body(Body::from(form))
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(response["active"], false);
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
    let (sid, native, _, oauth) = start_session(&user, None).await;
    let revocable_oauth = delegated_token(&user, Some(&sid));
    verify_credential_sources(router, &native, &revocable_oauth).await;
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
