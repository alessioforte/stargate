use super::*;
use axum::{
    Extension, Router,
    body::Body,
    middleware::from_fn,
    routing::{get, post},
};
use http::StatusCode;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

fn context() -> db::ent::TrustedAuditContext {
    db::ent::TrustedAuditContext::admin_control_plane(
        db::ent::TrustedAdminActor::admin("01JZ000000000000000000000A"),
        db::ent::TrustedAuditRequest::from_http(
            ulid::Ulid::generate().to_string(),
            None,
            None,
            None,
        ),
    )
}

async fn body(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

async fn wire_response(response: Response) -> (StatusCode, http::HeaderMap, Vec<u8>) {
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        body.collect().await.unwrap().to_bytes().to_vec(),
    )
}

async fn revoked_key() -> String {
    let secret = pw::generate_admin_key();
    let key = crate::data::db::create_admin_key(
        &pw::hash_api_key(&secret),
        None,
        vec!["users:read".into()],
        None,
        context(),
    )
    .await
    .unwrap();
    crate::data::db::revoke_admin_key(&key.id, context())
        .await
        .unwrap();
    secret
}

fn request(secret: &str, listener: Option<&str>) -> Request {
    let mut req = Request::builder()
        .method("POST")
        .uri("/admin/service-accounts")
        .header("content-type", "application/json")
        .header("x-api-key", secret)
        // Headers are never a source for listener identity.
        .header("x-forwarded-host", "internal")
        .body(Body::from(r#"{"name":"listener-test-account"}"#))
        .unwrap();
    if let Some(listener) = listener {
        let mut connection = crate::srv::test_connection("127.0.0.1:12345".parse().unwrap());
        Arc::get_mut(&mut connection.listener).unwrap().name = listener.into();
        req.extensions_mut().insert(connection);
    }
    req
}

fn router() -> Router {
    Router::new()
        .route(
            "/admin/service-accounts",
            post(service_accounts::create_service_account),
        )
        .layer(from_fn(extract_admin_authorization))
        .layer(Extension(Arc::new(crate::gate::test_support::gate(
            gate::cfg::Config::default(),
        ))))
}

#[tokio::test]
async fn restricted_keys_are_checked_before_mutation_and_principal_limits() {
    crate::data::db::with_test_database(async {
        let secret = pw::generate_admin_key();
        let key = crate::data::db::create_admin_key(
            &pw::hash_api_key(&secret),
            None,
            vec!["service_accounts:create".into()],
            Some(vec!["internal".into(), "automation".into()]),
            context(),
        )
        .await
        .unwrap();
        let revoked = revoked_key().await;
        // No gateway extension: rejection must precede principal limiting too.
        let app = Router::new()
            .route(
                "/admin/service-accounts",
                post(service_accounts::create_service_account),
            )
            .route("/admin/me", get(me::get_admin_me))
            .layer(from_fn(extract_admin_authorization));
        for listener in [Some("admin"), Some("default"), None] {
            let before = crate::data::db::count_service_accounts().await.unwrap();
            for path in ["/admin/service-accounts", "/admin/me"] {
                let call = |key: &str| {
                    let mut req = request(key, listener);
                    *req.uri_mut() = path.parse().unwrap();
                    if path == "/admin/me" {
                        *req.method_mut() = http::Method::GET;
                    }
                    req
                };
                let unknown = wire_response(
                    app.clone()
                        .oneshot(call("unknown-admin-key"))
                        .await
                        .unwrap(),
                )
                .await;
                assert_eq!(unknown.0, StatusCode::UNAUTHORIZED);
                assert_eq!(
                    serde_json::from_slice::<Value>(&unknown.2).unwrap()["code"],
                    "auth.token_invalid"
                );
                for rejected in [&secret, &revoked] {
                    let response =
                        wire_response(app.clone().oneshot(call(rejected)).await.unwrap()).await;
                    assert_eq!(response, unknown);
                }
            }
            assert_eq!(
                crate::data::db::count_service_accounts().await.unwrap(),
                before
            );
        }
        for listener in ["internal", "automation"] {
            let response = router()
                .oneshot(request(&secret, Some(listener)))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
        }
        // An unknown name is accepted at creation, but cannot authorize this listener.
        let mut changed = key;
        changed.set_allowed_listeners(Some(vec!["future".into()]));
        crate::data::db::update_admin_key(changed.clone(), context())
            .await
            .unwrap();
        let response = router()
            .oneshot(request(&secret, Some("internal")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        changed.set_allowed_listeners(None);
        crate::data::db::update_admin_key(changed, context())
            .await
            .unwrap();
        let response = router()
            .oneshot(request(&secret, Some("admin")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    })
    .await;
}

#[tokio::test]
async fn unrestricted_keys_work_on_every_listener() {
    crate::data::db::with_test_database(async {
        let secret = pw::generate_admin_key();
        crate::data::db::create_admin_key(
            &pw::hash_api_key(&secret),
            None,
            vec!["service_accounts:create".into()],
            None,
            context(),
        )
        .await
        .unwrap();
        for listener in ["internal", "admin", "default"] {
            let response = router()
                .oneshot(request(&secret, Some(listener)))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
        }
    })
    .await;
}

#[tokio::test]
async fn oauth_admin_operations_reject_keys_on_disallowed_listeners() {
    crate::data::db::with_test_database(async {
        let secret = pw::generate_admin_key();
        crate::data::db::create_admin_key(
            &pw::hash_api_key(&secret),
            None,
            vec![
                "oauth_tokens:introspect".into(),
                "oauth_tokens:revoke".into(),
            ],
            Some(vec!["internal".into()]),
            context(),
        )
        .await
        .unwrap();
        let revoked = revoked_key().await;
        let app = Router::new()
            .route("/oauth/introspect", post(crate::api::oidc::post_introspect))
            .route("/oauth/revoke", post(crate::api::oidc::post_revoke))
            .layer(from_fn(extract_authorization));
        for path in ["/oauth/introspect", "/oauth/revoke"] {
            // The bearer variant also exercises the invalid-client challenge header.
            for bearer in [false, true] {
                let call = |key: &str, listener| {
                    let mut req = request(key, listener);
                    *req.uri_mut() = path.parse().unwrap();
                    req.headers_mut().insert(
                        http::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded".parse().unwrap(),
                    );
                    if bearer {
                        req.headers_mut().insert(
                            http::header::AUTHORIZATION,
                            "Bearer invalid-bearer".parse().unwrap(),
                        );
                    }
                    *req.body_mut() = Body::from("token=not-a-token");
                    req
                };
                for listener in [Some("admin"), Some("default"), None] {
                    let unknown = wire_response(
                        app.clone()
                            .oneshot(call("unknown-admin-key", listener))
                            .await
                            .unwrap(),
                    )
                    .await;
                    assert_eq!(
                        unknown.0,
                        if bearer {
                            StatusCode::UNAUTHORIZED
                        } else {
                            StatusCode::FORBIDDEN
                        }
                    );
                    let payload: Value = serde_json::from_slice(&unknown.2).unwrap();
                    assert_eq!(
                        payload["error"],
                        if bearer {
                            "invalid_client"
                        } else {
                            "insufficient_scope"
                        }
                    );
                    assert!(payload.get("code").is_none());
                    if bearer {
                        assert!(unknown.1.contains_key(http::header::WWW_AUTHENTICATE));
                    }
                    for rejected in [&secret, &revoked] {
                        let response = wire_response(
                            app.clone().oneshot(call(rejected, listener)).await.unwrap(),
                        )
                        .await;
                        assert_eq!(response, unknown);
                    }
                }
                let response = app
                    .clone()
                    .oneshot(call(&secret, Some("internal")))
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }
        }
    })
    .await;
}

#[tokio::test]
async fn super_admin_can_create_read_and_replace_key_settings() {
    crate::data::db::with_test_database(async {
        // Management requires the existing super-admin authorization and audit context.
        let auth = AdminAuthorization::super_admin_user(
            "01JZ000000000000000000000A".into(),
            AdminAuthenticationKind::Session,
            Box::default(),
        );
        let app = Router::new()
            .route(
                "/admin/admin-keys",
                post(admin_keys::create_admin_key).get(admin_keys::get_admin_keys),
            )
            .route(
                "/admin/admin-keys/{id}",
                axum::routing::put(admin_keys::update_admin_key).get(admin_keys::get_admin_key),
            )
            .layer(Extension(auth))
            .layer(Extension(context()));
        let call = |method: &str, path: &str, payload: Value| {
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap()
        };
        let response = app.clone().oneshot(call("POST", "/admin/admin-keys", json!({
            "permissions": ["users:read"], "allowedListeners": ["Future", "internal", "INTERNAL"]
        }))).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let created = body(response).await;
        assert_eq!(created["allowedListeners"], json!(["future", "internal"]));
        let path = format!("/admin/admin-keys/{}", created["id"].as_str().unwrap());
        let response = app
            .clone()
            .oneshot(call("GET", &path, Value::Null))
            .await
            .unwrap();
        assert_eq!(
            body(response).await["allowedListeners"],
            created["allowedListeners"]
        );
        let response = app
            .clone()
            .oneshot(call("PUT", &path, json!({"permissions": ["users:read"]})))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body(response).await["code"], "request.invalid_json");
        let response = app
            .clone()
            .oneshot(call("GET", &path, Value::Null))
            .await
            .unwrap();
        assert_eq!(
            body(response).await["allowedListeners"],
            json!(["future", "internal"])
        );
        let response = app
            .clone()
            .oneshot(call(
                "PUT",
                &path,
                json!({"permissions":["users:read"], "allowedListeners":null}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body(response).await["allowedListeners"].is_null());
        for listeners in [json!([]), json!(["bad-name"]), json!([""])] {
            let response = app
                .clone()
                .oneshot(call(
                    "PUT",
                    &path,
                    json!({"permissions":["users:read"], "allowedListeners":listeners}),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(body(response).await["code"], "admin_key.listeners_invalid");
        }
        let response = app
            .oneshot(call("GET", "/admin/admin-keys", Value::Null))
            .await
            .unwrap();
        assert!(body(response).await["data"][0]["allowedListeners"].is_null());
    })
    .await;
}

#[tokio::test]
async fn listener_rejections_log_metadata_without_credentials() {
    const CHILD: &str = "STARGATE_TEST_ADMIN_KEY_REJECTION_LOG_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Tracing's callsite cache is shared with parallel tests' subscribers.
        let output = crate::conf::child_test::run(
            "api::admin::listener_tests::listener_rejections_log_metadata_without_credentials",
            |command| {
                command.env(CHILD, "1");
            },
        );
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    use std::{io::Write, sync::Mutex};
    #[derive(Clone)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let capture = Capture(buffer.clone());
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || capture.clone())
            .finish(),
    )
    .unwrap();
    crate::data::db::with_test_database(async {
        let secret = pw::generate_admin_key();
        let hash = pw::hash_api_key(&secret);
        let key = crate::data::db::create_admin_key(
            &hash,
            None,
            vec!["users:read".into()],
            Some(vec!["internal".into()]),
            context(),
        )
        .await
        .unwrap();
        let resolution = resolve_authorization(None, Some(secret.clone()), Some("admin"))
            .await
            .unwrap();
        assert!(!resolution.authentication.authenticated);
        assert!(resolution.authorization.is_none());
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert!(!output.contains(&secret));
        assert!(!output.contains(&hash));
        let events = output
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        let rejection = events
            .iter()
            .find(|event| event["fields"]["message"] == "Admin key rejected on disallowed listener")
            .unwrap();
        assert_eq!(rejection["fields"]["admin_key_id"], key.id);
        assert_eq!(rejection["fields"]["listener"], "admin");
    })
    .await;
}
