pub mod access_control_rules;
pub mod admin_keys;
pub mod api_keys;
pub mod configurations;
pub mod health;
pub mod oauth_clients;
pub mod organizations;
pub mod service_accounts;
pub mod users;

use crate::err::{ErrorResponse, HttpError};
use crate::etc::ext::RequestExt;
use crate::etc::jwt::jwt_config;
use crate::etc::reqctx::take_audit_context_from;
use crate::etc::sub::Subject;
use axum::Json;
use axum::extract::{FromRequest, FromRequestParts, Path, Query, Request};
use axum::middleware::Next;
use axum::response::Response;
use std::collections::HashSet;
use store::Store;

pub const SUPER_ADMIN: &str = "super_admin";

#[derive(Clone, Default, Debug)]
pub struct Grants(pub HashSet<String>);

impl Grants {
    pub fn has_any(&self, needed: &[&str]) -> bool {
        needed.iter().any(|g| self.0.contains(*g))
    }
}

fn admin_key_grants<'a>(permissions: impl IntoIterator<Item = &'a String>) -> HashSet<String> {
    permissions
        .into_iter()
        .filter(|permission| permission.as_str() != SUPER_ADMIN)
        .cloned()
        .collect()
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

async fn grant_super_admin_if_allowed(
    grants: &mut HashSet<String>,
    user_id: &str,
    sid: Option<&str>,
) -> bool {
    match crate::db::is_super_admin_user_id(user_id).await {
        Ok(true) => {
            if super_admin_step_up_satisfied(sid.unwrap_or_default(), user_id).await {
                grants.insert(SUPER_ADMIN.to_string());
                true
            } else {
                false
            }
        }
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

pub async fn extract_grants(mut req: Request, next: Next) -> Response {
    let mut ctx = take_audit_context_from(req.extensions_mut());
    let mut grants: HashSet<String> = HashSet::new();

    if let Some(token) = req.get_token() {
        let jwt = jwt_config();
        if let Ok(claims) = jwt.validate_session_access_token(&token) {
            match crate::act::token_revocation::is_revoked(&claims).await {
                Ok(false) => {
                    let sid = claims.sid.clone().unwrap_or_default();
                    let store = crate::etc::store::use_store();
                    let session = store.get::<Subject>(&sid).await.unwrap_or(None);
                    if session.is_some()
                        && let Some(user_id) = claims.sub_id.as_deref()
                        && grant_super_admin_if_allowed(&mut grants, user_id, Some(&sid)).await
                    {
                        ctx = ctx.with_actor(db::ent::ActorType::Admin, Some(user_id.to_string()));
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
                    if oauth_client_is_trusted_admin_client(client_id).await
                        && let Some(user_id) = claims.sub_id.as_deref()
                        && grant_super_admin_if_allowed(&mut grants, user_id, claims.sid.as_deref())
                            .await
                    {
                        ctx = ctx.with_actor(db::ent::ActorType::Admin, Some(user_id.to_string()));
                    }
                }
                Ok(true) => {}
                Err(err) => {
                    tracing::error!("Failed to check token revocation state: {}", err);
                }
            }
        }
    }

    if grants.is_empty()
        && let Some(raw_key) = req.get_api_key()
    {
        let hash = pw::hash_api_key(&raw_key);
        if let Ok(Some(key)) = crate::db::get_admin_key_by_hash(&hash).await
            && !key.revoked
        {
            ctx = ctx.with_actor(db::ent::ActorType::AdminKey, Some(key.id));
            grants = admin_key_grants(key.permissions.iter());
        }
    }

    req.extensions_mut().insert(Grants(grants));
    req.extensions_mut().insert(ctx);
    next.run(req).await
}

pub async fn extract_path<T>(req: &mut Request) -> Result<T, ErrorResponse>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    let (mut parts, body) =
        std::mem::replace(req, Request::new(axum::body::Body::empty())).into_parts();
    let Path(value) = Path::<T>::from_request_parts(&mut parts, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.to_string())))?;
    *req = Request::from_parts(parts, body);
    Ok(value)
}

pub fn extract_query<T>(req: &Request) -> Result<T, ErrorResponse>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    let Query(value) = Query::<T>::try_from_uri(req.uri())
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.to_string())))?;
    Ok(value)
}

pub async fn extract_json<T>(req: Request) -> Result<T, ErrorResponse>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    let Json(value) = Json::<T>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;
    Ok(value)
}

#[macro_export]
macro_rules! require_grants {
    ($req:expr, $($grant:expr),+ $(,)?) => {{
        let grants = $req
            .extensions()
            .get::<$crate::api::admin::Grants>()
            .cloned()
            .unwrap_or_default();
        if !grants.has_any(&[$($grant),+]) {
            return Err($crate::err::ErrorResponse::from(
                $crate::err::HttpError::Forbidden(
                    "Insufficient permissions".to_string(),
                ),
            ));
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::{SUPER_ADMIN, admin_key_grants};

    #[test]
    fn admin_key_grants_do_not_include_super_admin() {
        let permissions = [
            "users".to_string(),
            SUPER_ADMIN.to_string(),
            "organizations".to_string(),
        ];
        let grants = admin_key_grants(permissions.iter());

        assert!(grants.contains("users"));
        assert!(grants.contains("organizations"));
        assert!(!grants.contains(SUPER_ADMIN));
    }
}

pub fn router() -> axum::Router {
    use axum::middleware::from_fn;
    use axum::routing::{get, post, put};

    axum::Router::new()
        .route("/admin/health", get(health::get_admin_health))
        .route(
            "/admin/users",
            get(users::get_users).post(users::create_user),
        )
        .route(
            "/admin/users/organizations/{org_id}",
            get(users::get_organization_users),
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
        .layer(from_fn(extract_grants))
}
