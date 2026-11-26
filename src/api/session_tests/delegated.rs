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

    // A header credential is not CSRF-able: the origin guard leaves it alone
    // even when the origin and the Fetch Metadata are untrusted.
    let response = send(
        router,
        Request::get("/account/profile")
            .header(header::AUTHORIZATION, bearer(native))
            .header(header::ORIGIN, "https://evil.example.com")
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert!(response["id"].is_string());

    // A malformed Authorization header never falls back to the cookie, even
    // though the cookie alone would authenticate.
    for value in ["Bearer", "Basic dXNlcjpwYXNz"] {
        let response = send(
            router,
            Request::get("/account/profile")
                .header(header::AUTHORIZATION, value)
                .header(header::COOKIE, format!("jwt={native}"))
                .body(Body::empty())
                .unwrap(),
            StatusCode::UNAUTHORIZED,
        )
        .await;
        assert_eq!(response["code"], "auth.token_missing", "{value}");
    }

    // Routes without a header credential stay behind the full guard even when
    // they carry an Authorization header: there the header authenticates
    // nothing, so it cannot turn the guard off.
    let valid_bearer = bearer(native);
    for authorization in ["x", valid_bearer.as_str()] {
        for (method, path, content_type, body) in [
            (
                "POST",
                "/account/login",
                "application/json",
                r#"{"username":"x","password":"y"}"#,
            ),
            (
                "POST",
                "/account/login/otp/email",
                "application/json",
                r#"{"email":"x@example.test"}"#,
            ),
            (
                "POST",
                "/oauth/login",
                "application/x-www-form-urlencoded",
                "username=x&password=y",
            ),
            (
                "POST",
                "/account/credentials",
                "application/json",
                r#"{"email":"x@example.test"}"#,
            ),
            (
                "PUT",
                "/account/refresh-token",
                "application/json",
                r#"{"refresh_token":"x"}"#,
            ),
            (
                "POST",
                "/signup",
                "application/json",
                r#"{"email":"x@example.test"}"#,
            ),
            (
                "POST",
                "/oauth/interactions/01JZ0000000000000000000000/consent",
                "application/json",
                r#"{"approve":true}"#,
            ),
        ] {
            let response = send(
                router,
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::CONTENT_TYPE, content_type)
                    .header(header::ORIGIN, "https://evil.example.com")
                    .header(header::AUTHORIZATION, authorization)
                    .body(Body::from(body))
                    .unwrap(),
                StatusCode::FORBIDDEN,
            )
            .await;
            assert_eq!(
                response["code"], "auth.untrusted_origin",
                "{method} {path} with {authorization}"
            );
        }
    }

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
