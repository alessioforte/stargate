pub mod admin_keys;
pub mod api_keys;
pub mod configurations;
pub mod health;
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

pub async fn extract_grants(mut req: Request, next: Next) -> Response {
    let mut ctx = take_audit_context_from(req.extensions_mut());
    let mut grants: HashSet<String> = HashSet::new();

    if let Some(token) = req.get_token() {
        let jwt = jwt_config();
        if let Some(claims) = jwt
            .validate_token(&token)
            .ok()
            .filter(|c| c.typ.as_deref() == Some("bearer"))
        {
            let sid = claims.sid.clone().unwrap_or_default();
            let store = crate::etc::store::use_store();
            let session = store.get::<Subject>(&sid).await.unwrap_or(None);
            if session.is_some()
                && let Some(user_id) = claims.sub_id.as_deref()
            {
                match crate::db::is_super_admin_user_id(user_id).await {
                    Ok(true) => {
                        ctx = ctx.with_actor(db::ent::ActorType::Admin, Some(user_id.to_string()));
                        grants.insert(SUPER_ADMIN.to_string());
                    }
                    Ok(false) => {}
                    Err(err) => {
                        tracing::error!("Failed to resolve super admin grants: {}", err);
                    }
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
            grants = key.permissions.iter().map(|p| p.to_string()).collect();
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

pub fn router() -> axum::Router {
    use axum::middleware::from_fn;
    use axum::routing::{get, put};

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
        .layer(from_fn(extract_grants))
}
