use super::*;
use axum::{Extension, Router, body::Body, middleware::from_fn, routing::post};
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
        for listener in [Some("admin"), Some("default"), None] {
            let before = crate::data::db::count_service_accounts().await.unwrap();
            // No gateway extension: rejection must precede principal limiting too.
            let response = Router::new()
                .route(
                    "/admin/service-accounts",
                    post(service_accounts::create_service_account),
                )
                .layer(from_fn(extract_admin_authorization))
                .oneshot(request(&secret, listener))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(body(response).await["code"], "admin_key.listener_forbidden");
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
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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
        for path in ["/oauth/introspect", "/oauth/revoke"] {
            let app = Router::new()
                .route(path, post(|| async { StatusCode::NO_CONTENT }))
                .layer(from_fn(extract_authorization));
            let mut req = request(&secret, Some("admin"));
            *req.uri_mut() = path.parse().unwrap();
            let response = app.clone().oneshot(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            let payload = body(response).await;
            assert_eq!(payload["error"], "insufficient_scope");
            assert!(payload.get("code").is_none());
            let mut req = request(&secret, Some("internal"));
            *req.uri_mut() = path.parse().unwrap();
            assert_eq!(
                app.oneshot(req).await.unwrap().status(),
                StatusCode::NO_CONTENT
            );
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
