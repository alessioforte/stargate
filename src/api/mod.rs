pub mod account;
pub mod admin;
pub mod admin_app;
pub mod auth_app;
pub mod docs;
pub mod gateway;
pub mod health;
pub mod oidc;
pub mod signup;
pub mod social;

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::health::get_health,
        crate::api::oidc::well_known::get_jwks,
        crate::api::oidc::well_known::get_oauth_metadata,
        crate::api::oidc::well_known::get_openid_configuration,
        crate::api::docs::get_api_doc,
        crate::api::signup::request::post_signup,
        crate::api::signup::verification::get_signup,
        crate::api::signup::complete::put_signup,
        crate::api::social::state::post_state,
        crate::api::oidc::authorize::get_authorize,
        crate::api::oidc::token::post_token,
        crate::api::oidc::userinfo::get_userinfo,
        crate::api::oidc::userinfo::post_userinfo,
        crate::api::oidc::token_ops::post_introspect,
        crate::api::oidc::token_ops::post_revoke,
        crate::api::social::github::get_github,
        crate::api::social::google::get_google,
        crate::api::account::login::post_login,
        crate::api::account::logout::delete_logout,
        crate::api::account::otp::passwordless::post_login_email_otp,
        crate::api::account::otp::passwordless::put_login_email_otp,
        crate::api::account::otp::login_mfa::put_login_mfa_challenge,
        crate::api::account::otp::mfa::get_mfa_methods,
        crate::api::account::otp::mfa::post_mfa_challenge,
        crate::api::account::otp::mfa::put_mfa_challenge,
        crate::api::account::password_policy::get_password_policy,
        crate::api::account::profile::get_profile,
        crate::api::account::refresh_token::put_refresh_token,
        crate::api::account::credentials::forgot::post_credentials,
        crate::api::account::credentials::reset::put_credentials,
        crate::api::admin::health::get_admin_health,
        crate::api::admin::users::get_users,
        crate::api::admin::users::get_super_admin_users,
        crate::api::admin::users::get_user,
        crate::api::admin::users::create_user,
        crate::api::admin::users::create_user_invitation,
        crate::api::admin::users::update_user,
        crate::api::admin::users::patch_user,
        crate::api::admin::users::update_user_attrs,
        crate::api::admin::users::patch_user_attrs,
        crate::api::admin::users::delete_user,
        crate::api::admin::users::get_user_organizations,
        crate::api::admin::users::get_organization_users,
        crate::api::admin::users::add_user_to_organization,
        crate::api::admin::users::remove_user_from_organization,
        crate::api::admin::organizations::get_organizations,
        crate::api::admin::organizations::get_organization,
        crate::api::admin::organizations::create_organization,
        crate::api::admin::organizations::update_organization,
        crate::api::admin::organizations::delete_organization,
        crate::api::admin::api_keys::get_api_keys,
        crate::api::admin::api_keys::get_api_key,
        crate::api::admin::api_keys::create_api_key,
        crate::api::admin::api_keys::delete_api_key,
        crate::api::admin::api_keys::revoke_api_key,
        crate::api::admin::api_keys::update_api_key_attrs,
        crate::api::admin::api_keys::patch_api_key_attrs,
        crate::api::admin::admin_keys::get_admin_keys,
        crate::api::admin::admin_keys::get_admin_key,
        crate::api::admin::admin_keys::create_admin_key,
        crate::api::admin::admin_keys::update_admin_key_permissions,
        crate::api::admin::admin_keys::revoke_admin_key,
        crate::api::admin::admin_keys::delete_admin_key,
        crate::api::admin::oauth_clients::get_oauth_clients,
        crate::api::admin::oauth_clients::get_oauth_client,
        crate::api::admin::oauth_clients::create_oauth_client,
        crate::api::admin::oauth_clients::update_oauth_client,
        crate::api::admin::oauth_clients::patch_oauth_client,
        crate::api::admin::oauth_clients::disable_oauth_client,
        crate::api::admin::oauth_clients::enable_oauth_client,
        crate::api::admin::oauth_clients::rotate_oauth_client_secret,
        crate::api::admin::oauth_clients::delete_oauth_client,
        crate::api::admin::service_accounts::get_service_accounts,
        crate::api::admin::service_accounts::get_service_account,
        crate::api::admin::service_accounts::create_service_account,
        crate::api::admin::service_accounts::update_service_account,
        crate::api::admin::service_accounts::delete_service_account,
        crate::api::admin::configurations::get_configurations,
        crate::api::admin::configurations::update_configurations,
        crate::api::admin::access_control_rules::get_access_control_rules,
        crate::api::admin::access_control_rules::update_access_control_rules,
        crate::api::admin::access_control_rules::validate_access_control_rules,
        crate::api::admin::access_control_rules::evaluate_access_control_rules,
    ),
    info(
        title = "Stargate APIs ✨",
        description = "APIs for user authentication and management in Stargate.",
    )
)]
pub struct ApiDoc;

pub fn router() -> axum::Router {
    use axum::routing::get;

    axum::Router::new()
        .route("/health", get(health::get_health))
        .route("/docs", get(docs::get_api_doc))
        .merge(signup::router())
        .merge(oidc::router())
        .merge(social::router())
        .merge(account::router())
        .merge(auth_app::router())
        .merge(admin_app::router())
        .merge(admin::router())
}

#[cfg(test)]
mod tests {
    use super::router;
    use axum::body::Body;
    use http::{
        Method, Request, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn send(uri: &str) -> http::Response<Body> {
        router()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn send_req(req: Request<Body>) -> http::Response<Body> {
        router().oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn health_returns_json() {
        let resp = send("/health").await;
        // /health pings DB which is not initialized in tests; expect 503.
        assert!(matches!(
            resp.status(),
            StatusCode::OK | StatusCode::SERVICE_UNAVAILABLE
        ));
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "stargate");
    }

    #[tokio::test]
    async fn jwks_returns_200() {
        let resp = send("/.well-known/jwks.json").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(
            resp.headers().get(CACHE_CONTROL).unwrap(),
            "public, max-age=300"
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["keys"].is_array());
    }

    #[tokio::test]
    async fn oauth_authorization_server_metadata_returns_200() {
        let resp = send("/.well-known/oauth-authorization-server").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["issuer"].is_string());
        assert!(
            json["authorization_endpoint"]
                .as_str()
                .unwrap()
                .ends_with("/oauth/authorize")
        );
        assert!(
            json["token_endpoint"]
                .as_str()
                .unwrap()
                .ends_with("/oauth/token")
        );
        assert!(json["introspection_endpoint"].is_string());
        assert!(json["revocation_endpoint"].is_string());
        assert!(json["jwks_uri"].is_string());
        assert_eq!(
            json["response_types_supported"],
            serde_json::json!(["code"])
        );
        assert_eq!(
            json["grant_types_supported"],
            serde_json::json!(["authorization_code", "client_credentials", "refresh_token"])
        );
        assert_eq!(
            json["token_endpoint_auth_methods_supported"],
            serde_json::json!(["client_secret_basic", "client_secret_post", "none"])
        );
        assert_eq!(
            json["code_challenge_methods_supported"],
            serde_json::json!(["S256"])
        );
    }

    #[tokio::test]
    async fn openid_configuration_returns_200() {
        let resp = send("/.well-known/openid-configuration").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["issuer"].is_string());
        assert!(
            json["authorization_endpoint"]
                .as_str()
                .unwrap()
                .ends_with("/oauth/authorize")
        );
        assert!(
            json["userinfo_endpoint"]
                .as_str()
                .unwrap()
                .ends_with("/oauth/userinfo")
        );
        assert_eq!(
            json["response_types_supported"],
            serde_json::json!(["code"])
        );
        assert_eq!(
            json["grant_types_supported"],
            serde_json::json!(["authorization_code", "client_credentials", "refresh_token"])
        );
        assert_eq!(
            json["scopes_supported"],
            serde_json::json!(["openid", "email", "profile", "offline_access"])
        );
        assert_eq!(
            json["code_challenge_methods_supported"],
            serde_json::json!(["S256"])
        );
    }

    #[tokio::test]
    async fn docs_returns_openapi_json() {
        let resp = send("/docs").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["openapi"].is_string());
        assert!(json["paths"]["/health"].is_object());
        assert!(json["paths"]["/.well-known/jwks.json"].is_object());
        assert!(json["paths"]["/.well-known/oauth-authorization-server"].is_object());
        assert!(json["paths"]["/.well-known/openid-configuration"].is_object());
        assert!(json["paths"]["/signup"].is_object());
        assert!(json["paths"]["/oauth/state"].is_object());
        assert!(json["paths"]["/oauth/authorize"].is_object());
        assert!(json["paths"]["/oauth/token"].is_object());
        assert!(json["paths"]["/oauth/userinfo"].is_object());
        assert!(json["paths"]["/oauth/introspect"].is_object());
        assert!(json["paths"]["/oauth/revoke"].is_object());
        assert!(json["paths"]["/oauth/github"].is_object());
        assert!(json["paths"]["/oauth/google"].is_object());
        assert!(json["paths"]["/account/login"].is_object());
        assert!(json["paths"]["/account/logout"].is_object());
        assert!(json["paths"]["/account/profile"].is_object());
        assert!(json["paths"]["/account/refresh-token"].is_object());
        assert!(json["paths"]["/account/credentials"].is_object());
        assert!(json["paths"]["/admin/health"].is_object());
        assert!(json["paths"]["/admin/users"].is_object());
        assert!(json["paths"]["/admin/users/invitations"].is_object());
        assert!(json["paths"]["/admin/users/super-admins"].is_object());
        assert!(json["paths"]["/admin/users/{id}"].is_object());
        assert!(json["paths"]["/admin/users/{id}/attrs"].is_object());
        assert!(json["paths"]["/admin/users/{id}/organizations"].is_object());
        assert!(json["paths"]["/admin/users/{id}/organizations/{org_id}"].is_object());
        assert!(json["paths"]["/admin/users/organizations/{org_id}"].is_object());
        assert!(json["paths"]["/admin/organizations"].is_object());
        assert!(json["paths"]["/admin/organizations/{id}"].is_object());
        assert!(json["paths"]["/admin/api-keys"].is_object());
        assert!(json["paths"]["/admin/api-keys/{id}"].is_object());
        assert!(json["paths"]["/admin/api-keys/{id}/revoke"].is_object());
        assert!(json["paths"]["/admin/api-keys/{id}/attrs"].is_object());
        assert!(json["paths"]["/admin/admin-keys"].is_object());
        assert!(json["paths"]["/admin/admin-keys/{id}"].is_object());
        assert!(json["paths"]["/admin/admin-keys/{id}/revoke"].is_object());
        assert!(json["paths"]["/admin/admin-keys/{id}/permissions"].is_object());
        assert!(json["paths"]["/admin/oauth/clients"].is_object());
        assert!(json["paths"]["/admin/oauth/clients/{client_id}"].is_object());
        assert!(json["paths"]["/admin/oauth/clients/{client_id}/disable"].is_object());
        assert!(json["paths"]["/admin/oauth/clients/{client_id}/enable"].is_object());
        assert!(json["paths"]["/admin/oauth/clients/{client_id}/rotate-secret"].is_object());
        assert!(json["paths"]["/admin/service-accounts"].is_object());
        assert!(json["paths"]["/admin/service-accounts/{id}"].is_object());
        assert!(json["paths"]["/admin/configurations"].is_object());
        assert!(json["paths"]["/admin/access-control/rules"].is_object());
        assert!(json["paths"]["/admin/access-control/rules/validate"].is_object());
        assert!(json["paths"]["/admin/access-control/rules/evaluate"].is_object());
    }

    #[tokio::test]
    async fn admin_health_missing_grant_forbidden() {
        let resp = send("/admin/health").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_users_missing_grant_forbidden() {
        let resp = send("/admin/users").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_user_invitations_missing_grant_forbidden() {
        let resp = send_req(
            Request::builder()
                .method(Method::POST)
                .uri("/admin/users/invitations")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_user_by_id_missing_grant_forbidden() {
        let resp = send("/admin/users/abc").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_super_admin_users_missing_grant_forbidden() {
        let resp = send("/admin/users/super-admins").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_organizations_missing_grant_forbidden() {
        let resp = send("/admin/organizations").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_organization_by_id_missing_grant_forbidden() {
        let resp = send("/admin/organizations/abc").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_api_keys_missing_grant_forbidden() {
        let resp = send("/admin/api-keys").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_api_key_by_id_missing_grant_forbidden() {
        let resp = send("/admin/api-keys/abc").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_admin_keys_missing_grant_forbidden() {
        let resp = send("/admin/admin-keys").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_admin_key_by_id_missing_grant_forbidden() {
        let resp = send("/admin/admin-keys/abc").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_oauth_clients_missing_grant_forbidden() {
        let resp = send("/admin/oauth/clients").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_oauth_client_by_id_missing_grant_forbidden() {
        let resp = send("/admin/oauth/clients/client_abc").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_service_accounts_missing_grant_forbidden() {
        let resp = send("/admin/service-accounts").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_configurations_missing_grant_forbidden() {
        let resp = send("/admin/configurations").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_access_control_rules_missing_grant_forbidden() {
        let resp = send("/admin/access-control/rules").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn signup_get_missing_token_rejected() {
        let resp = send("/signup").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn signup_post_missing_body_rejected() {
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/signup")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::empty())
            .unwrap();
        let resp = send_req(req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn oauth_github_missing_query_rejected() {
        let resp = send("/oauth/github").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn oauth_google_missing_query_rejected() {
        let resp = send("/oauth/google").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn oauth_authorize_missing_query_rejected() {
        let resp = send("/oauth/authorize").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn oauth_token_missing_form_rejected() {
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/oauth/token")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::empty())
            .unwrap();
        let resp = send_req(req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid_request");
        assert!(json.get("message").is_none());
    }

    #[tokio::test]
    async fn oauth_userinfo_missing_token_unauthorized() {
        let resp = send("/oauth/userinfo").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn oauth_introspect_missing_grant_forbidden() {
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/oauth/introspect")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("token=abc"))
            .unwrap();
        let resp = send_req(req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "insufficient_scope");
        assert!(json.get("message").is_none());
    }

    #[tokio::test]
    async fn oauth_revoke_missing_grant_forbidden() {
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/oauth/revoke")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("token=abc"))
            .unwrap();
        let resp = send_req(req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "insufficient_scope");
        assert!(json.get("message").is_none());
    }

    #[tokio::test]
    async fn account_logout_missing_token_unauthorized() {
        let req = Request::builder()
            .method(http::Method::DELETE)
            .uri("/account/logout")
            .body(Body::empty())
            .unwrap();
        let resp = send_req(req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn account_profile_missing_token_unauthorized() {
        let resp = send("/account/profile").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn account_refresh_token_missing_body_rejected() {
        let req = Request::builder()
            .method(http::Method::PUT)
            .uri("/account/refresh-token")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::empty())
            .unwrap();
        let resp = send_req(req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn signup_put_missing_content_type_rejected() {
        let req = Request::builder()
            .method(http::Method::PUT)
            .uri("/signup")
            .body(Body::from("{}"))
            .unwrap();
        let resp = send_req(req).await;
        // axum Json rejects missing/wrong content-type with 415
        assert!(matches!(
            resp.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE | StatusCode::BAD_REQUEST
        ));
    }
}
