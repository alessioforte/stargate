//! Exercise the HTTP mutation handlers with persisted principals and targets.
use super::*;
use authorization::Permission;
use axum::{
    Router,
    body::Body,
    routing::{delete, post, put},
};
use db::{
    DbStore,
    ent::{
        CredentialType, OrganizationMembershipMutation, OutboxEventFilter, Profile,
        TrustedAdminActor, TrustedAuditContext, TrustedAuditRequest, User,
    },
};
use http::StatusCode;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn context(actor: &str) -> TrustedAuditContext {
    TrustedAuditContext::admin_control_plane(
        TrustedAdminActor::admin(actor),
        TrustedAuditRequest::from_http(ulid::Ulid::generate().to_string(), None, None, None),
    )
}

fn router() -> Router {
    Router::new()
        .route("/admin/users", post(users::create_user))
        .route(
            "/admin/users/invitations",
            post(users::create_user_invitation),
        )
        .route(
            "/admin/users/{id}",
            put(users::update_user)
                .patch(users::patch_user)
                .delete(users::delete_user),
        )
        .route(
            "/admin/users/{id}/attrs",
            put(users::update_user_attrs).patch(users::patch_user_attrs),
        )
        .route(
            "/admin/users/{id}/organizations/{org_id}",
            put(users::add_user_to_organization).delete(users::remove_user_from_organization),
        )
        .route(
            "/admin/users/{user_id}/sessions",
            delete(sessions::delete_user_sessions),
        )
        .route(
            "/admin/sessions/{session_id}",
            delete(sessions::delete_session),
        )
        .route("/admin/api-keys", post(api_keys::create_api_key))
        .route("/admin/api-keys/{id}", delete(api_keys::delete_api_key))
        .route(
            "/admin/api-keys/{id}/attrs",
            put(api_keys::update_api_key_attrs).patch(api_keys::patch_api_key_attrs),
        )
        .route("/admin/api-keys/{id}/revoke", put(api_keys::revoke_api_key))
        .route(
            "/admin/organizations/{id}",
            put(organizations::update_organization).delete(organizations::delete_organization),
        )
        .route(
            "/admin/oauth/clients",
            post(oauth_clients::create_oauth_client),
        )
        .route(
            "/admin/oauth/clients/{client_id}",
            put(oauth_clients::update_oauth_client)
                .patch(oauth_clients::patch_oauth_client)
                .delete(oauth_clients::delete_oauth_client),
        )
        .route(
            "/admin/oauth/clients/{client_id}/disable",
            put(oauth_clients::disable_oauth_client),
        )
        .route(
            "/admin/oauth/clients/{client_id}/enable",
            put(oauth_clients::enable_oauth_client),
        )
        .route(
            "/admin/oauth/clients/{client_id}/rotate-secret",
            put(oauth_clients::rotate_oauth_client_secret),
        )
}

struct Fixture {
    target: User,
    actor: User,
    key: db::ent::ApiKey,
    org_id: String,
    empty_org_id: String,
    sid: String,
    user_secret: String,
    admin_secret: String,
}

impl Fixture {
    async fn new(protected: bool) -> Self {
        let svc = crate::data::db::service();
        let actor = svc
            .create_super_admin_user(
                Profile::new("operator@example.test".into(), "operator".into()),
                CredentialType::Password,
                "operator-password-hash",
                TrustedAuditContext::system_control_plane(),
            )
            .await
            .unwrap();
        let profile = Profile::new("target@example.test".into(), "target".into());
        let target = if protected {
            svc.create_super_admin_user(
                profile,
                CredentialType::Password,
                "target-password-hash",
                TrustedAuditContext::system_control_plane(),
            )
            .await
            .unwrap()
        } else {
            svc.create_user(
                profile,
                CredentialType::Password,
                "target-password-hash",
                None,
                context(&actor.id),
            )
            .await
            .unwrap()
        };
        let org = svc
            .create_organization("Target org", None, None, context(&actor.id))
            .await
            .unwrap();
        let empty_org = svc
            .create_organization("Other org", None, None, context(&actor.id))
            .await
            .unwrap();
        svc.add_user_to_organization(
            &target.id,
            &org.id,
            "owner",
            OrganizationMembershipMutation::Create,
            context(&actor.id),
        )
        .await
        .unwrap();
        let user_secret = pw::generate_api_key();
        let key = svc
            .create_user_api_key(
                &target.id,
                &pw::hash_api_key(&user_secret),
                "target",
                None,
                Some(&org.id),
                context(&actor.id),
            )
            .await
            .unwrap();
        let sid = ulid::Ulid::generate().to_string();
        crate::data::store::use_store()
            .set(
                &sid,
                &crate::iam::auth::subject::Subject::from(target.clone()),
                Some(3600),
            )
            .await
            .unwrap();
        crate::iam::sess::registry::register_session(
            crate::iam::sess::registry::NewSession {
                user_id: &target.id,
                sid: &sid,
                org_id: Some(&org.id),
                authenticated_at_unix: 1,
                amr: vec!["pwd".into()],
            },
            "refresh-jti",
            3600,
        )
        .await
        .unwrap();
        let admin_secret = pw::generate_api_key();
        svc.create_admin_key(
            &pw::hash_api_key(&admin_secret),
            Some("all capabilities".into()),
            Permission::ALL
                .iter()
                .map(|p| p.as_str().to_owned())
                .collect(),
            context(&actor.id),
        )
        .await
        .unwrap();
        Self {
            target,
            actor,
            key,
            org_id: org.id,
            empty_org_id: empty_org.id,
            sid,
            user_secret,
            admin_secret,
        }
    }

    async fn request(&self, method: &str, uri: &str, body: Value, super_admin: bool) -> Request {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let auth = if super_admin {
            AdminAuthorization::super_admin_user(
                self.actor.id.clone(),
                AdminAuthenticationKind::OAuth,
                Box::default(),
            )
        } else {
            let resolution = resolve_authorization(None, Some(self.admin_secret.clone())).await;
            assert!(resolution.authentication.authenticated);
            let auth = resolution.authorization.unwrap();
            assert!(!auth.super_admin);
            assert_eq!(auth.permissions().count(), Permission::ALL.len());
            auth
        };
        let actor = match &auth.principal {
            AdminPrincipal::User { user_id, .. } => TrustedAdminActor::admin(user_id),
            AdminPrincipal::AdminKey { key_id } => TrustedAdminActor::admin_key(key_id),
        };
        let audit_context = TrustedAuditContext::admin_control_plane(
            actor,
            TrustedAuditRequest::from_http(ulid::Ulid::generate().to_string(), None, None, None),
        );
        req.extensions_mut().insert(auth.principal.clone());
        req.extensions_mut().insert(auth);
        req.extensions_mut().insert(audit_context);
        req.extensions_mut()
            .insert(std::sync::Arc::new(crate::gate::test_support::gate(
                gate::cfg::Config::default(),
            )));
        req
    }

    fn operation(&self, name: &str) -> (&'static str, String, Value, StatusCode) {
        let user = format!("/admin/users/{}", self.target.id);
        let key = format!("/admin/api-keys/{}", self.key.id);
        let membership = format!("{user}/organizations/{}", self.org_id);
        match name {
            "profile_put" => (
                "PUT",
                user,
                json!({"email":"new@example.test","nickname":"new","givenName":"Updated","phoneNumber":"123"}),
                StatusCode::OK,
            ),
            "profile_patch" => (
                "PATCH",
                user,
                json!({"email":"new@example.test","nickname":"new","picture":"https://example.test/image"}),
                StatusCode::OK,
            ),
            "attrs_put" => (
                "PUT",
                format!("{user}/attrs"),
                json!({"attrs":{"mfa":{"enabled":false}}}),
                StatusCode::OK,
            ),
            "attrs_patch" => (
                "PATCH",
                format!("{user}/attrs"),
                json!({"attrs":{"default_org":"other"}}),
                StatusCode::OK,
            ),
            "key_create" => (
                "POST",
                "/admin/api-keys".into(),
                json!({"userId":self.target.id,"label":"new"}),
                StatusCode::CREATED,
            ),
            "key_delete" => ("DELETE", key, json!({}), StatusCode::OK),
            "key_revoke" => ("PUT", format!("{key}/revoke"), json!({}), StatusCode::OK),
            "key_attrs_put" => (
                "PUT",
                format!("{key}/attrs"),
                json!({"attrs":{"access":"changed"}}),
                StatusCode::OK,
            ),
            "key_attrs_patch" => (
                "PATCH",
                format!("{key}/attrs"),
                json!({"attrs":{"access":"changed"}}),
                StatusCode::OK,
            ),
            "session" => (
                "DELETE",
                format!("/admin/sessions/{}", self.sid),
                json!({}),
                StatusCode::OK,
            ),
            "all_sessions" => (
                "DELETE",
                format!("{user}/sessions"),
                json!({}),
                StatusCode::OK,
            ),
            "membership_add" => (
                "PUT",
                format!("{user}/organizations/{}", self.empty_org_id),
                json!({"role":"admin"}),
                StatusCode::OK,
            ),
            "membership_update" => ("PUT", membership, json!({"role":"member"}), StatusCode::OK),
            "membership_delete" => ("DELETE", membership, json!({}), StatusCode::OK),
            "organization_update" => (
                "PUT",
                format!("/admin/organizations/{}", self.org_id),
                json!({"name":"Updated","attrs":{"passwordPolicy":{"maxAgeDays":1}}}),
                StatusCode::OK,
            ),
            "organization_delete" => (
                "DELETE",
                format!("/admin/organizations/{}", self.org_id),
                json!({}),
                StatusCode::NO_CONTENT,
            ),
            "user_delete" => ("DELETE", user, json!({}), StatusCode::OK),
            _ => panic!("unknown operation {name}"),
        }
    }

    async fn snapshot(&self) -> Value {
        let svc = crate::data::db::service();
        let mut memberships = Vec::new();
        for org in svc.get_all_organizations(100, 0).await.unwrap() {
            memberships.push(
                json!({"org":org,"members":svc.get_organization_users(&org.id).await.unwrap()}),
            );
        }
        json!({
            "users":svc.get_all_users(100,0).await.unwrap(),
            "superAdmins":svc.get_super_admin_users().await.unwrap(),
            "keys":svc.get_all_api_keys(100,0).await.unwrap(),
            "memberships":memberships,
            "clients":svc.get_all_oauth_clients(100,0).await.unwrap(),
            "credentials":svc.get_credential(&self.target.id, CredentialType::Password).await.unwrap(),
            "audit":svc.query_outbox_events(&OutboxEventFilter::default(),1000,0).await.unwrap().into_iter().map(|row| row.payload).collect::<Vec<_>>(),
            "session":crate::iam::sess::registry::get_session(&self.sid).await.unwrap(),
            "subject":crate::data::store::use_store().get::<Value>(&self.sid).await.unwrap(),
        })
    }
}

const OPERATIONS: &[&str] = &[
    "profile_put",
    "profile_patch",
    "attrs_put",
    "attrs_patch",
    "key_create",
    "key_delete",
    "key_revoke",
    "key_attrs_put",
    "key_attrs_patch",
    "session",
    "all_sessions",
    "membership_add",
    "membership_update",
    "membership_delete",
    "organization_update",
    "organization_delete",
    "user_delete",
];

#[tokio::test]
async fn every_capability_still_denies_mutations_of_super_admins_without_side_effects() {
    crate::data::db::with_test_database(async {
        let f = Fixture::new(true).await;
        for name in OPERATIONS {
            let before = f.snapshot().await;
            let (method, uri, body, _) = f.operation(name);
            let response = router()
                .oneshot(f.request(method, &uri, body, false).await)
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{name}");
            let body: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(
                body["code"], "user.super_admin_mutation_forbidden",
                "{name}"
            );
            assert_eq!(
                f.snapshot().await,
                before,
                "rejected {name} changed persistence"
            );
        }
    })
    .await;
}

#[tokio::test]
async fn capabilities_continue_to_manage_regular_users_and_super_admins_keep_control() {
    for (protected, super_admin) in [(false, false), (true, true)] {
        for name in OPERATIONS
            .iter()
            .filter(|name| **name != "user_delete" || !protected)
        {
            crate::data::db::with_test_database(async {
                let f = Fixture::new(protected).await;
                let before = f.snapshot().await;
                let (method, uri, body, status) = f.operation(name);
                let response = router()
                    .oneshot(f.request(method, &uri, body, super_admin).await)
                    .await
                    .unwrap();
                let actual = response.status();
                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                assert_eq!(
                    actual,
                    status,
                    "{name}, super_admin={super_admin}: {}",
                    String::from_utf8_lossy(&bytes)
                );
                assert_ne!(
                    f.snapshot().await,
                    before,
                    "accepted {name} did not mutate its target"
                );
            })
            .await;
        }
    }
}

#[tokio::test]
async fn console_super_admin_can_edit_their_own_profile_and_deletion_stays_forbidden() {
    crate::data::db::with_test_database(async {
        let mut f = Fixture::new(true).await;
        f.actor = f.target.clone();
        for name in ["profile_put", "profile_patch", "attrs_put", "attrs_patch"] {
            let (method, uri, body, status) = f.operation(name);
            let response = router()
                .oneshot(f.request(method, &uri, body, true).await)
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{name}");
        }
        let before = f.snapshot().await;
        let (method, uri, body, _) = f.operation("user_delete");
        let response = router()
            .oneshot(f.request(method, &uri, body, true).await)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["code"], "user.super_admin_delete_forbidden");
        assert_eq!(f.snapshot().await, before);
    })
    .await;
}

#[tokio::test]
async fn creation_invitation_and_login_identifiers_cannot_replace_a_super_admin() {
    crate::data::db::with_test_database(async {
        let f = Fixture::new(true).await;
        let cases = [
            ("POST", "/admin/users".to_owned(), json!({"email":" TARGET@EXAMPLE.TEST ","password":"unused"}), StatusCode::CONFLICT),
            ("POST", "/admin/users/invitations".to_owned(), json!({"email":" TARGET@EXAMPLE.TEST ","nickname":"attacker"}), StatusCode::CONFLICT),
            ("POST", "/admin/users".to_owned(), json!({"email":"attacker@example.test","nickname":"TARGET","password":"unused"}), StatusCode::CONFLICT),
            ("POST", "/admin/users".to_owned(), json!({"email":"attacker@example.test","nickname":"target@example.test","password":"unused"}), StatusCode::BAD_REQUEST),
            ("PATCH", format!("/admin/users/{}",f.actor.id), json!({"email":"attacker@example.test"}), StatusCode::FORBIDDEN),
        ];
        for (method,uri,payload,status) in cases {
            let before = f.snapshot().await;
            let response = router().oneshot(f.request(method,&uri,payload,false).await).await.unwrap();
            assert_eq!(response.status(),status,"{method} {uri}");
            assert_eq!(f.snapshot().await,before);
        }
        let login = crate::data::db::get_user_by_login(" TARGET@EXAMPLE.TEST ").await.unwrap().unwrap();
        assert_eq!(login.id,f.target.id);
        assert_eq!(crate::data::db::get_user_by_login("TARGET").await.unwrap().unwrap().id,f.target.id);
        assert!(crate::data::db::get_user_by_email("attacker@example.test").await.unwrap().is_none());
        // The failed email change leaves recovery at the existing mailbox.
        // A request to the attacker's address sends no mail and creates no reset.
        let recovery_email = format!("attacker-{}@example.test",ulid::Ulid::generate());
        let _ = crate::api::account::credentials::forgot::post_credentials(Json(crate::api::account::credentials::forgot::ForgotPasswordRequestBody {
            email: recovery_email.clone(),
        })).await.unwrap();
        assert!(crate::iam::pwd::reset::get_change_password_request(&recovery_email).await.unwrap().is_none());
    }).await;
}

#[tokio::test]
async fn even_a_super_admin_owned_user_api_key_is_not_an_admin_credential() {
    crate::data::db::with_test_database(async {
        let f = Fixture::new(true).await;
        let resolution = resolve_authorization(None, Some(f.user_secret.clone())).await;
        assert!(resolution.authorization.is_none());
        let mut req = Request::new(Body::empty());
        req.headers_mut()
            .insert("x-api-key", f.user_secret.parse().unwrap());
        let identity = crate::iam::auth::verification::verify_api_key(&req)
            .await
            .unwrap();
        assert_eq!(identity.subject().unwrap().id, f.key.id);
        assert_eq!(
            identity.subject().unwrap().sub_type,
            crate::iam::auth::subject::SubjectType::ApiKey
        );
    })
    .await;
}

fn client_payload(first_party: bool) -> Value {
    json!({"clientId":"test-client","name":"Test client","grantTypes":["authorization_code"],
        "tokenEndpointAuthMethod":"client_secret_basic","responseTypes":["code"],
        "redirectUris":["https://console.example.test/callback"],"scopes":["openid"],
        "attrs":{"first_party":first_party}})
}

#[tokio::test]
async fn service_account_keys_keep_their_capability_based_management() {
    crate::data::db::with_test_database(async {
        let mut f = Fixture::new(false).await;
        let account =
            crate::data::db::create_service_account("Worker", None, None, context(&f.actor.id))
                .await
                .unwrap();
        f.key = crate::data::db::create_service_account_api_key(
            &account.id,
            &pw::hash_api_key(&pw::generate_api_key()),
            "Worker",
            None,
            context(&f.actor.id),
        )
        .await
        .unwrap();
        assert!(
            crate::data::db::get_api_key_user_id(&f.key.id)
                .await
                .unwrap()
                .is_none()
        );
        for name in [
            "key_attrs_put",
            "key_attrs_patch",
            "key_revoke",
            "key_delete",
        ] {
            let (method, uri, body, status) = f.operation(name);
            let response = router()
                .oneshot(f.request(method, &uri, body, false).await)
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{name}");
        }
    })
    .await;
}

#[tokio::test]
async fn admin_keys_cannot_control_or_create_clients_trusted_by_the_admin_api() {
    for (first_party, super_admin) in [(true, false), (false, false), (true, true)] {
        for (method, suffix) in [
            ("PUT", ""),
            ("PATCH", ""),
            ("DELETE", ""),
            ("PUT", "/enable"),
            ("PUT", "/disable"),
            ("PUT", "/rotate-secret"),
        ] {
            crate::data::db::with_test_database(async {
                let f = Fixture::new(true).await;
                let response = router()
                    .oneshot(
                        f.request(
                            "POST",
                            "/admin/oauth/clients",
                            client_payload(first_party),
                            true,
                        )
                        .await,
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::CREATED);
                let before = f.snapshot().await;
                let uri = format!("/admin/oauth/clients/test-client{suffix}");
                // Clearing first_party in the same update cannot evade the check.
                let response = router()
                    .oneshot(
                        f.request(method, &uri, client_payload(false), super_admin)
                            .await,
                    )
                    .await
                    .unwrap();
                let status = response.status();
                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                assert_eq!(
                    status,
                    if first_party && !super_admin {
                        StatusCode::FORBIDDEN
                    } else {
                        StatusCode::OK
                    },
                    "{method} {suffix}: {}",
                    String::from_utf8_lossy(&bytes)
                );
                if first_party && !super_admin {
                    assert_eq!(f.snapshot().await, before);
                }
            })
            .await;
        }
    }
    crate::data::db::with_test_database(async {
        let f = Fixture::new(true).await;
        let before = f.snapshot().await;
        let response = router()
            .oneshot(
                f.request("POST", "/admin/oauth/clients", client_payload(true), false)
                    .await,
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(f.snapshot().await, before);
        let response = router()
            .oneshot(
                f.request("POST", "/admin/oauth/clients", client_payload(false), false)
                    .await,
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        for method in ["PUT", "PATCH"] {
            let before = f.snapshot().await;
            let response = router()
                .oneshot(
                    f.request(
                        method,
                        "/admin/oauth/clients/test-client",
                        client_payload(true),
                        false,
                    )
                    .await,
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "promote via {method}"
            );
            assert_eq!(f.snapshot().await, before);
        }
    })
    .await;
}
