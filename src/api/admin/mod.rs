pub mod access_control_rules;
pub mod admin_keys;
pub mod api_keys;
pub mod authorization;
pub mod configurations;
pub mod health;
pub mod me;
pub mod oauth_clients;
pub mod organizations;
pub mod outbox_events;
pub mod overview;
pub mod service_accounts;
pub mod sessions;
pub mod users;

use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::ext::RequestExt;
use crate::etc::jwt::jwt_config;
use crate::etc::reqctx::{audit_request_from, take_trusted_audit_context_from};
use crate::etc::sub::Subject;
use axum::Json;
use axum::extract::{FromRequest, FromRequestParts, Path, Query, Request};
use axum::middleware::Next;
use axum::response::Response;
use store::Store;

use authorization::{AdminAuthorization, parse_stored_permissions};

#[derive(Clone, Copy, Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdminAuthenticationKind {
    Session,
    OAuth,
    AdminKey,
}

#[derive(Clone, Debug)]
pub enum AdminPrincipal {
    User {
        user_id: String,
        kind: AdminAuthenticationKind,
        claims: Box<jwt::Claims>,
    },
    AdminKey {
        key_id: String,
    },
}

impl AdminPrincipal {
    pub fn user_id(&self) -> Option<&str> {
        match self {
            Self::User { user_id, .. } => Some(user_id),
            Self::AdminKey { .. } => None,
        }
    }

    pub fn claims(&self) -> Option<&jwt::Claims> {
        match self {
            Self::User { claims, .. } => Some(claims.as_ref()),
            Self::AdminKey { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AdminAuthenticationState {
    pub credential_present: bool,
    pub authenticated: bool,
}

async fn super_admin_step_up_satisfied(sid: &str, user_id: &str) -> bool {
    if !crate::act::otp::admin_step_up_required() {
        return true;
    }

    if sid.is_empty() {
        return false;
    }

    match crate::act::otp::has_valid_mfa_verification(
        sid,
        user_id,
        crate::act::otp::ADMIN_STEP_UP_PURPOSE,
    )
    .await
    {
        Ok(satisfied) => satisfied,
        Err(err) => {
            tracing::error!("Failed to check admin MFA step-up state: {}", err);
            false
        }
    }
}

async fn grant_super_admin_if_allowed(user_id: &str, sid: Option<&str>) -> bool {
    match crate::db::is_super_admin_user_id(user_id).await {
        Ok(true) => super_admin_step_up_satisfied(sid.unwrap_or_default(), user_id).await,
        Ok(false) => false,
        Err(err) => {
            tracing::error!("Failed to resolve super admin grants: {}", err);
            false
        }
    }
}

async fn oauth_client_is_trusted_admin_client(client_id: &str) -> bool {
    let client = match crate::db::get_oauth_client_by_client_id(client_id).await {
        Ok(Some(client)) => client,
        Ok(None) => return false,
        Err(err) => {
            tracing::error!("Failed to resolve OAuth client for admin grants: {}", err);
            return false;
        }
    };

    client.enabled && oidc::consent::client_is_first_party(&client.attrs)
}

#[derive(Clone, Copy)]
enum AuditRouteBoundary {
    Application,
    AdminControlPlane,
}

struct AuthorizationResolution {
    authentication: AdminAuthenticationState,
    actor: Option<db::ent::TrustedAdminActor>,
    authorization: Option<AdminAuthorization>,
}

async fn resolve_authorization(
    token: Option<String>,
    raw_key: Option<String>,
) -> AuthorizationResolution {
    let credential_present = token.is_some() || raw_key.is_some();
    let mut authenticated = false;
    let mut actor = None;
    let mut authorization = None;

    if let Some(token) = token {
        let jwt = jwt_config();
        if let Ok(claims) = jwt.validate_session_access_token(&token) {
            match crate::act::token_revocation::is_revoked(&claims).await {
                Ok(false) => {
                    let sid = claims.sid.clone().unwrap_or_default();
                    let store = crate::etc::store::use_store();
                    let session = store.get::<Subject>(&sid).await.unwrap_or(None);
                    if let Some(subject) = session.as_ref()
                        && subject.sub_type == crate::etc::sub::SubjectType::User
                        && claims.sub_id.as_deref() == Some(subject.id.as_str())
                    {
                        match crate::act::sessions::ensure_session_registered(
                            &subject.id,
                            &sid,
                            subject.org_id.as_deref(),
                            claims.auth_time.unwrap_or(claims.iat) as u64,
                        )
                        .await
                        {
                            Ok(()) => authenticated = true,
                            Err(err) => {
                                tracing::error!("Failed to register admin session: {}", err);
                            }
                        }
                    }
                    if authenticated
                        && let Some(user_id) = claims.sub_id.as_deref()
                        && grant_super_admin_if_allowed(user_id, Some(&sid)).await
                    {
                        let user_id = user_id.to_string();
                        actor = Some(db::ent::TrustedAdminActor::admin(&user_id));
                        authorization = Some(AdminAuthorization::super_admin_user(
                            user_id,
                            AdminAuthenticationKind::Session,
                            Box::new(claims),
                        ));
                    }
                }
                Ok(true) => {}
                Err(err) => {
                    tracing::error!("Failed to check token revocation state: {}", err);
                }
            }
        } else if let Ok(claims) = jwt.validate_oauth_access_token(&token, None) {
            match crate::act::token_revocation::is_revoked(&claims).await {
                Ok(false) => {
                    let client_id = claims.azp.as_deref().unwrap_or_default();
                    let user_id = claims.sub_id.as_deref();
                    let active_session = match (claims.sid.as_deref(), user_id) {
                        (Some(sid), Some(user_id)) => {
                            match crate::act::sessions::get_active_session(sid, user_id).await {
                                Ok(session) => session,
                                Err(err) => {
                                    tracing::error!(
                                        "Failed to resolve backing OAuth session: {}",
                                        err
                                    );
                                    None
                                }
                            }
                        }
                        // Access tokens minted before stable sessions were
                        // introduced remain valid until their normal expiry.
                        (None, _) => None,
                        _ => None,
                    };
                    authenticated = claims.sid.is_none() || active_session.is_some();
                    let step_up_sid = claims.sid.as_deref();
                    if oauth_client_is_trusted_admin_client(client_id).await
                        && authenticated
                        && let Some(user_id) = user_id
                        && grant_super_admin_if_allowed(user_id, step_up_sid).await
                    {
                        let user_id = user_id.to_string();
                        actor = Some(db::ent::TrustedAdminActor::admin(&user_id));
                        authorization = Some(AdminAuthorization::super_admin_user(
                            user_id,
                            AdminAuthenticationKind::OAuth,
                            Box::new(claims),
                        ));
                    }
                }
                Ok(true) => {}
                Err(err) => {
                    tracing::error!("Failed to check token revocation state: {}", err);
                }
            }
        }
    }

    if authorization.is_none()
        && let Some(raw_key) = raw_key
    {
        let hash = pw::hash_api_key(&raw_key);
        if let Ok(Some(key)) = crate::db::get_admin_key_by_hash(&hash).await
            && !key.revoked
        {
            authenticated = true;
            let key_id = key.id.clone();
            actor = Some(db::ent::TrustedAdminActor::admin_key(&key_id));
            authorization = Some(AdminAuthorization::admin_key(
                AdminPrincipal::AdminKey {
                    key_id: key_id.clone(),
                },
                parse_stored_permissions(&key_id, key.permissions.iter()),
            ));
        }
    }

    AuthorizationResolution {
        authentication: AdminAuthenticationState {
            credential_present,
            authenticated,
        },
        actor,
        authorization,
    }
}

fn install_audit_context(
    extensions: &mut http::Extensions,
    actor: Option<db::ent::TrustedAdminActor>,
    boundary: AuditRouteBoundary,
) {
    let request = audit_request_from(extensions);

    // Never inherit actor or scope from an upstream extension. Authentication
    // above is the only source of an admin principal, while this middleware's
    // route binding is the only source of the admin scope.
    extensions.remove::<db::ent::TrustedAuditContext>();

    let Some(actor) = actor else {
        return;
    };

    let context = match boundary {
        AuditRouteBoundary::Application => {
            db::ent::TrustedAuditContext::application(actor.audit_actor(), request)
        }
        AuditRouteBoundary::AdminControlPlane => {
            db::ent::TrustedAuditContext::admin_control_plane(actor, request)
        }
    };
    extensions.insert(context);
}

async fn extract_authorization_for(
    mut req: Request,
    next: Next,
    boundary: AuditRouteBoundary,
) -> Response {
    let token = req.get_token();
    let raw_key = req.get_api_key();
    let resolution = resolve_authorization(token, raw_key).await;
    install_audit_context(req.extensions_mut(), resolution.actor, boundary);
    req.extensions_mut().insert(resolution.authentication);
    if let Some(authorization) = resolution.authorization {
        req.extensions_mut().insert(authorization.principal.clone());
        req.extensions_mut().insert(authorization);
    }
    next.run(req).await
}

/// Authorization extraction reused by non-admin OAuth token operations. An admin
/// principal does not imply control-plane scope outside `/admin/*`.
pub async fn extract_authorization(req: Request, next: Next) -> Response {
    extract_authorization_for(req, next, AuditRouteBoundary::Application).await
}

/// `/admin/*` authentication and its route-owned, immutable control-plane
/// audit boundary.
pub async fn extract_admin_authorization(req: Request, next: Next) -> Response {
    extract_authorization_for(req, next, AuditRouteBoundary::AdminControlPlane).await
}

pub fn take_admin_audit_context(
    extensions: &mut http::Extensions,
) -> Result<db::ent::TrustedAuditContext, ErrorResponse> {
    take_trusted_audit_context_from(extensions)
        .ok_or_else(|| ErrorResponse::internal("missing authenticated admin audit context"))
}

pub async fn extract_path<T>(req: &mut Request) -> Result<T, ErrorResponse>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    let (mut parts, body) =
        std::mem::replace(req, Request::new(axum::body::Body::empty())).into_parts();
    let Path(value) = Path::<T>::from_request_parts(&mut parts, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalid).with_message(error.to_string())
        })?;
    *req = Request::from_parts(parts, body);
    Ok(value)
}

pub fn extract_query<T>(req: &Request) -> Result<T, ErrorResponse>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    let Query(value) = Query::<T>::try_from_uri(req.uri()).map_err(|error| {
        ErrorResponse::new(ErrorCode::RequestInvalidQuery).with_message(error.to_string())
    })?;
    Ok(value)
}

pub async fn extract_json<T>(req: Request) -> Result<T, ErrorResponse>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    let Json(value) = Json::<T>::from_request(req, &()).await.map_err(|error| {
        ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
    })?;
    Ok(value)
}

/// Like `extract_json`, but an empty body yields `None` instead of a 400,
/// for endpoints whose body is optional.
pub async fn extract_optional_json<T>(req: Request) -> Result<Option<T>, ErrorResponse>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    const MAX_OPTIONAL_BODY_BYTES: usize = 64 * 1024;

    let bytes = axum::body::to_bytes(req.into_body(), MAX_OPTIONAL_BODY_BYTES)
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalid).with_message(error.to_string())
        })?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let value = serde_json::from_slice(&bytes).map_err(|error| {
        ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.to_string())
    })?;
    Ok(Some(value))
}

pub fn router() -> axum::Router {
    use axum::middleware::from_fn;
    use axum::routing::{delete, get, post, put};

    axum::Router::new()
        .route("/admin/health", get(health::get_admin_health))
        .route("/admin/me", get(me::get_admin_me))
        .route("/admin/overview", get(overview::get_admin_overview))
        .route("/admin/sessions", get(sessions::get_sessions))
        .route(
            "/admin/sessions/{session_id}",
            delete(sessions::delete_session),
        )
        .route(
            "/admin/users",
            get(users::get_users).post(users::create_user),
        )
        .route(
            "/admin/users/invitations",
            post(users::create_user_invitation),
        )
        .route(
            "/admin/users/organizations/{org_id}",
            get(users::get_organization_users),
        )
        .route(
            "/admin/users/super-admins",
            get(users::get_super_admin_users),
        )
        .route(
            "/admin/users/{id}",
            get(users::get_user)
                .put(users::update_user)
                .patch(users::patch_user)
                .delete(users::delete_user),
        )
        .route(
            "/admin/users/{id}/attrs",
            put(users::update_user_attrs).patch(users::patch_user_attrs),
        )
        .route(
            "/admin/users/{id}/organizations",
            get(users::get_user_organizations),
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
            "/admin/organizations",
            get(organizations::get_organizations).post(organizations::create_organization),
        )
        .route(
            "/admin/organizations/{id}",
            get(organizations::get_organization)
                .put(organizations::update_organization)
                .delete(organizations::delete_organization),
        )
        .route(
            "/admin/outbox-events",
            get(outbox_events::get_outbox_events),
        )
        .route(
            "/admin/outbox-events/{event_id}",
            get(outbox_events::get_outbox_event),
        )
        .route(
            "/admin/api-keys",
            get(api_keys::get_api_keys).post(api_keys::create_api_key),
        )
        .route(
            "/admin/api-keys/{id}",
            get(api_keys::get_api_key).delete(api_keys::delete_api_key),
        )
        .route("/admin/api-keys/{id}/revoke", put(api_keys::revoke_api_key))
        .route(
            "/admin/api-keys/{id}/attrs",
            put(api_keys::update_api_key_attrs).patch(api_keys::patch_api_key_attrs),
        )
        .route(
            "/admin/admin-keys",
            get(admin_keys::get_admin_keys).post(admin_keys::create_admin_key),
        )
        .route(
            "/admin/admin-keys/{id}",
            get(admin_keys::get_admin_key).delete(admin_keys::delete_admin_key),
        )
        .route(
            "/admin/admin-keys/{id}/revoke",
            put(admin_keys::revoke_admin_key),
        )
        .route(
            "/admin/admin-keys/{id}/permissions",
            put(admin_keys::update_admin_key_permissions),
        )
        .route(
            "/admin/oauth/clients",
            get(oauth_clients::get_oauth_clients).post(oauth_clients::create_oauth_client),
        )
        .route(
            "/admin/oauth/clients/{client_id}",
            get(oauth_clients::get_oauth_client)
                .put(oauth_clients::update_oauth_client)
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
        .route(
            "/admin/service-accounts",
            get(service_accounts::get_service_accounts)
                .post(service_accounts::create_service_account),
        )
        .route(
            "/admin/service-accounts/{id}",
            get(service_accounts::get_service_account)
                .put(service_accounts::update_service_account)
                .delete(service_accounts::delete_service_account),
        )
        .route(
            "/admin/configurations",
            get(configurations::get_configurations).put(configurations::update_configurations),
        )
        .route(
            "/admin/access-control/rules",
            get(access_control_rules::get_access_control_rules)
                .put(access_control_rules::update_access_control_rules),
        )
        .route(
            "/admin/access-control/rules/validate",
            post(access_control_rules::validate_access_control_rules),
        )
        .route(
            "/admin/access-control/rules/evaluate",
            post(access_control_rules::evaluate_access_control_rules),
        )
        .layer(from_fn(extract_admin_authorization))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use db::ent::{
        AuditScopeSelector, TrustedAdminActor, TrustedAuditActor, TrustedAuditContext,
        TrustedAuditRequest,
    };

    use crate::etc::reqctx::RequestContext;

    use super::{AuditRouteBoundary, install_audit_context};

    fn request_extensions() -> http::Extensions {
        let mut extensions = http::Extensions::new();
        extensions.insert(RequestContext::new(
            "01JZ000000000000000000000R".to_string(),
            Utc::now(),
            Some("192.0.2.20".parse().expect("test IP")),
            Some("test-agent".to_string()),
            Some("4bf92f3577b34da6a3ce929d0e0e4736".to_string()),
        ));
        extensions
    }

    #[test]
    fn every_admin_handler_uses_typed_authorization() {
        let sources = [
            (
                "access_control_rules",
                include_str!("access_control_rules.rs"),
            ),
            ("admin_keys", include_str!("admin_keys.rs")),
            ("api_keys", include_str!("api_keys.rs")),
            ("configurations", include_str!("configurations.rs")),
            ("health", include_str!("health.rs")),
            ("me", include_str!("me.rs")),
            ("oauth_clients", include_str!("oauth_clients.rs")),
            ("organizations", include_str!("organizations.rs")),
            ("outbox_events", include_str!("outbox_events.rs")),
            ("overview", include_str!("overview.rs")),
            ("service_accounts", include_str!("service_accounts.rs")),
            ("sessions", include_str!("sessions.rs")),
            ("users", include_str!("users.rs")),
        ];

        for (module, source) in sources {
            assert!(
                !source.contains("require_grants!"),
                "{module} uses the removed string-grant macro"
            );
            for handler in source.split("pub async fn ").skip(1) {
                let handler_name = handler.split('(').next().unwrap_or("unknown");
                assert!(
                    handler.contains("authorization::require")
                        || handler.contains("authorization::get"),
                    "admin handler {module}::{handler_name} does not use typed authorization"
                );
            }
        }
    }

    #[test]
    fn all_admin_resource_targets_receive_control_plane_scope() {
        let targets = [
            "user",
            "organization",
            "organization_membership",
            "oauth_client",
            "api_key",
            "admin_key",
            "service_account",
            "configuration",
            "access_control_rule",
        ];

        for target in targets {
            let mut extensions = request_extensions();
            install_audit_context(
                &mut extensions,
                Some(TrustedAdminActor::admin("01JZ000000000000000000000A")),
                AuditRouteBoundary::AdminControlPlane,
            );

            let context = extensions
                .get::<TrustedAuditContext>()
                .unwrap_or_else(|| panic!("missing admin audit context for {target}"));
            assert_eq!(
                context.boundary().scope(),
                &AuditScopeSelector::ControlPlane,
                "unexpected scope for {target}"
            );
        }
    }

    #[test]
    fn admin_authentication_replaces_spoofed_actor_and_scope() {
        let mut extensions = request_extensions();
        let spoofed = TrustedAuditContext::organization(
            TrustedAuditActor::user("forged-user"),
            TrustedAuditRequest::from_http("forged-request", None, None, None),
            Some("01JZ0000000000000000000001"),
            None,
        )
        .expect("valid test organization");
        extensions.insert(spoofed);
        install_audit_context(
            &mut extensions,
            Some(TrustedAdminActor::admin_key("01JZ000000000000000000000K")),
            AuditRouteBoundary::AdminControlPlane,
        );

        let context = extensions
            .get::<TrustedAuditContext>()
            .expect("authenticated context");
        assert_eq!(
            context.boundary().scope(),
            &AuditScopeSelector::ControlPlane
        );
        assert_eq!(context.actor().actor_type(), "admin_key");
        assert_eq!(context.actor().id(), Some("01JZ000000000000000000000K"));
        assert_eq!(
            context
                .request()
                .expect("request facts")
                .raw_request()
                .request_id,
            "01JZ000000000000000000000R"
        );
    }

    #[test]
    fn admin_role_outside_admin_routes_does_not_select_control_plane() {
        let mut extensions = request_extensions();
        install_audit_context(
            &mut extensions,
            Some(TrustedAdminActor::admin("01JZ000000000000000000000A")),
            AuditRouteBoundary::Application,
        );

        let context = extensions
            .get::<TrustedAuditContext>()
            .expect("authenticated context");
        assert_eq!(context.boundary().scope(), &AuditScopeSelector::Application);
        assert_eq!(context.actor().actor_type(), "admin");
    }

    #[test]
    fn failed_admin_authentication_drops_any_inherited_audit_context() {
        let mut extensions = request_extensions();
        extensions.insert(TrustedAuditContext::application(
            TrustedAuditActor::admin("forged-admin"),
            TrustedAuditRequest::from_http("forged-request", None, None, None),
        ));

        install_audit_context(&mut extensions, None, AuditRouteBoundary::AdminControlPlane);

        assert!(extensions.get::<TrustedAuditContext>().is_none());
    }
}
