use super::{SUPER_ADMIN, extract_json, extract_path, extract_query};
use crate::api::admin::take_admin_audit_context;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::msg::MessageResponse;
use crate::require_grants;
use axum::Json;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use serde::{Deserialize, Serialize};

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminKeySchema {
    pub id: String,
    pub label: Option<String>,
    pub permissions: Vec<String>,
    pub revoked: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAdminKeyResponse {
    pub id: String,
    pub label: Option<String>,
    pub permissions: Vec<String>,
    pub revoked: bool,
    pub created_at: String,
    pub updated_at: String,
    pub api_key: String,
}

#[derive(Deserialize, Debug, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListAdminKeysQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAdminKeyRequest {
    #[serde(default)]
    pub label: Option<String>,
    pub permissions: Vec<AdminKeyPermission>,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAdminKeyPermissionsRequest {
    pub permissions: Vec<AdminKeyPermission>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub enum AdminKeyPermission {
    #[serde(rename = "access_control")]
    AccessControl,
    #[serde(rename = "users")]
    Users,
    #[serde(rename = "organizations")]
    Organizations,
    #[serde(rename = "api_keys")]
    ApiKeys,
    #[serde(rename = "oauth_clients")]
    OAuthClients,
    #[serde(rename = "service_accounts")]
    ServiceAccounts,
    #[serde(rename = "configurations")]
    Configurations,
}

impl AdminKeyPermission {
    fn as_str(self) -> &'static str {
        match self {
            Self::AccessControl => "access_control",
            Self::Users => "users",
            Self::Organizations => "organizations",
            Self::ApiKeys => "api_keys",
            Self::OAuthClients => "oauth_clients",
            Self::ServiceAccounts => "service_accounts",
            Self::Configurations => "configurations",
        }
    }
}

fn admin_key_permissions_to_strings(permissions: &[AdminKeyPermission]) -> Vec<String> {
    permissions
        .iter()
        .map(|permission| permission.as_str().to_string())
        .collect()
}

fn validate_admin_key_permissions(permissions: &[AdminKeyPermission]) -> Result<(), ErrorResponse> {
    if permissions.is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Permissions list cannot be empty".to_string(),
        )));
    }

    Ok(())
}

#[utoipa::path(
    get,
    path = "/admin/admin-keys",
    tags = ["Admin", "Admin Keys"],
    params(ListAdminKeysQuery),
    responses(
        (status = 200, description = "List of admin keys retrieved successfully", body = PaginatedResponse<AdminKeySchema>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_admin_keys(req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN);

    let query: ListAdminKeysQuery = extract_query(&req)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);

    let keys = crate::db::get_all_admin_keys(limit, offset)
        .await
        .map_err(ErrorResponse::internal)?;
    let total = crate::db::count_admin_keys()
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(PaginatedResponse {
        data: keys,
        total,
        limit,
        offset,
    })
    .into_response())
}

#[utoipa::path(
    get,
    path = "/admin/admin-keys/{id}",
    tags = ["Admin", "Admin Keys"],
    params(("id" = String, Path, description = "Admin Key ID")),
    responses(
        (status = 200, description = "Admin key retrieved successfully", body = AdminKeySchema),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Admin key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_admin_key(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN);

    let id: String = extract_path(&mut req).await?;

    let key = crate::db::get_admin_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::NotFound(format!(
                "Admin key with id '{}' not found",
                id
            )))
        })?;

    Ok(Json(key).into_response())
}

#[utoipa::path(
    post,
    path = "/admin/admin-keys",
    tags = ["Admin", "Admin Keys"],
    request_body = CreateAdminKeyRequest,
    responses(
        (status = 201, description = "Admin key created successfully", body = CreateAdminKeyResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn create_admin_key(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let payload: CreateAdminKeyRequest = extract_json(req).await?;

    validate_admin_key_permissions(&payload.permissions)?;
    let permissions = admin_key_permissions_to_strings(&payload.permissions);

    let secret = pw::generate_admin_key();
    let key_hash = pw::hash_api_key(&secret);

    let admin_key = crate::db::create_admin_key(&key_hash, payload.label.clone(), permissions, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    let response = CreateAdminKeyResponse {
        id: admin_key.id,
        label: admin_key.label,
        permissions: admin_key.permissions.0,
        revoked: admin_key.revoked,
        created_at: admin_key.created_at.to_rfc3339(),
        updated_at: admin_key.updated_at.to_rfc3339(),
        api_key: secret,
    };

    Ok((StatusCode::CREATED, Json(response)).into_response())
}

#[utoipa::path(
    put,
    path = "/admin/admin-keys/{id}/permissions",
    tags = ["Admin", "Admin Keys"],
    params(("id" = String, Path, description = "Admin Key ID")),
    request_body = UpdateAdminKeyPermissionsRequest,
    responses(
        (status = 200, description = "Admin key permissions updated successfully", body = AdminKeySchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Admin key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_admin_key_permissions(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;
    let payload: UpdateAdminKeyPermissionsRequest = extract_json(req).await?;

    validate_admin_key_permissions(&payload.permissions)?;
    let permissions = admin_key_permissions_to_strings(&payload.permissions);

    let mut existing = crate::db::get_admin_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::NotFound(format!(
                "Admin key with id '{}' not found",
                id
            )))
        })?;

    if existing.revoked {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Cannot update a revoked admin key".to_string(),
        )));
    }

    existing.set_permissions(permissions);

    let updated = crate::db::update_admin_key(existing, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(updated).into_response())
}

#[utoipa::path(
    put,
    path = "/admin/admin-keys/{id}/revoke",
    tags = ["Admin", "Admin Keys"],
    params(("id" = String, Path, description = "Admin Key ID")),
    responses(
        (status = 200, description = "Admin key revoked successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Admin key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn revoke_admin_key(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;

    let key = crate::db::get_admin_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::NotFound(format!(
                "Admin key with id '{}' not found",
                id
            )))
        })?;

    if key.revoked {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Admin key is already revoked".to_string(),
        )));
    }

    crate::db::revoke_admin_key(&id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(
        "Admin key revoked successfully",
        "admin_key_revoked",
    ))
    .into_response())
}

#[utoipa::path(
    delete,
    path = "/admin/admin-keys/{id}",
    tags = ["Admin", "Admin Keys"],
    params(("id" = String, Path, description = "Admin Key ID")),
    responses(
        (status = 200, description = "Admin key deleted successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Admin key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn delete_admin_key(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;

    if crate::db::get_admin_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "Admin key with id '{}' not found",
            id
        ))));
    }

    crate::db::delete_admin_key(&id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(
        "Admin key deleted successfully",
        "admin_key_deleted",
    ))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::{
        AdminKeyPermission, CreateAdminKeyRequest, admin_key_permissions_to_strings,
        validate_admin_key_permissions,
    };

    #[test]
    fn validates_admin_key_resource_permissions() {
        let permissions = vec![AdminKeyPermission::Users, AdminKeyPermission::Organizations];

        assert!(validate_admin_key_permissions(&permissions).is_ok());
    }

    #[test]
    fn rejects_empty_admin_key_permissions() {
        assert!(validate_admin_key_permissions(&[]).is_err());
    }

    #[test]
    fn serializes_admin_key_permissions_to_grant_strings() {
        let permissions = vec![
            AdminKeyPermission::ApiKeys,
            AdminKeyPermission::ServiceAccounts,
        ];

        assert_eq!(
            admin_key_permissions_to_strings(&permissions),
            vec!["api_keys".to_string(), "service_accounts".to_string()]
        );
    }

    #[test]
    fn rejects_super_admin_admin_key_permission() {
        let payload = serde_json::json!({
            "permissions": ["users", "super_admin"]
        });

        assert!(serde_json::from_value::<CreateAdminKeyRequest>(payload).is_err());
    }
}
