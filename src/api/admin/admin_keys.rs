use super::{
    authorization::{
        self, Permission, canonical_permission_strings, canonical_permissions,
        parse_stored_permissions,
    },
    extract_json, extract_path, extract_query,
};
use crate::api::admin::take_admin_audit_context;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::msg::{MessageCode, MessageResponse};
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
    pub permissions: Vec<Permission>,
    pub revoked: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAdminKeyResponse {
    pub id: String,
    pub label: Option<String>,
    pub permissions: Vec<Permission>,
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
    #[schema(value_type = Vec<Permission>)]
    pub permissions: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAdminKeyPermissionsRequest {
    #[schema(value_type = Vec<Permission>)]
    pub permissions: Vec<String>,
}

fn validate_admin_key_permissions(
    permissions: &[String],
) -> Result<Vec<Permission>, ErrorResponse> {
    if permissions.is_empty() {
        return Err(ErrorResponse::new(ErrorCode::AdminKeyPermissionsRequired));
    }

    let permissions = permissions
        .iter()
        .map(|permission| {
            Permission::parse(permission).ok_or_else(|| {
                ErrorResponse::new(ErrorCode::AdminKeyPermissionInvalid)
                    .with_param("permission", permission.clone())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(canonical_permissions(permissions))
}

fn admin_key_permissions(key: &db::ent::AdminKey) -> Vec<Permission> {
    canonical_permissions(parse_stored_permissions(&key.id, key.permissions.iter()))
}

fn admin_key_schema(key: db::ent::AdminKey) -> AdminKeySchema {
    let permissions = admin_key_permissions(&key);
    AdminKeySchema {
        id: key.id,
        label: key.label,
        permissions,
        revoked: key.revoked,
        created_at: key.created_at.to_rfc3339(),
        updated_at: key.updated_at.to_rfc3339(),
    }
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
    authorization::require_super_admin(&req)?;

    let query: ListAdminKeysQuery = extract_query(&req)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);

    let keys = crate::db::get_all_admin_keys(limit, offset)
        .await
        .map_err(ErrorResponse::internal)?
        .into_iter()
        .map(admin_key_schema)
        .collect();
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
    authorization::require_super_admin(&req)?;

    let id: String = extract_path(&mut req).await?;

    let key = crate::db::get_admin_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::new(ErrorCode::AdminKeyNotFound).with_param("id", id.clone())
        })?;

    Ok(Json(admin_key_schema(key)).into_response())
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
    authorization::require_super_admin(&req)?;

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let payload: CreateAdminKeyRequest = extract_json(req).await?;

    let permissions = validate_admin_key_permissions(&payload.permissions)?;
    let stored_permissions = canonical_permission_strings(permissions.iter().copied());

    let secret = pw::generate_admin_key();
    let key_hash = pw::hash_api_key(&secret);

    let admin_key =
        crate::db::create_admin_key(&key_hash, payload.label.clone(), stored_permissions, ctx)
            .await
            .map_err(ErrorResponse::internal)?;

    let response = CreateAdminKeyResponse {
        id: admin_key.id,
        label: admin_key.label,
        permissions,
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
    authorization::require_super_admin(&req)?;

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;
    let payload: UpdateAdminKeyPermissionsRequest = extract_json(req).await?;

    let permissions =
        canonical_permission_strings(validate_admin_key_permissions(&payload.permissions)?);

    let mut existing = crate::db::get_admin_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::new(ErrorCode::AdminKeyNotFound).with_param("id", id.clone())
        })?;

    if existing.revoked {
        return Err(ErrorResponse::new(ErrorCode::AdminKeyRevoked));
    }

    existing.set_permissions(permissions);

    let updated = crate::db::update_admin_key(existing, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(admin_key_schema(updated)).into_response())
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
    authorization::require_super_admin(&req)?;

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;

    let key = crate::db::get_admin_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::new(ErrorCode::AdminKeyNotFound).with_param("id", id.clone())
        })?;

    if key.revoked {
        return Err(ErrorResponse::new(ErrorCode::AdminKeyAlreadyRevoked));
    }

    crate::db::revoke_admin_key(&id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(MessageCode::AdminKeyRevoked)).into_response())
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
    authorization::require_super_admin(&req)?;

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;

    if crate::db::get_admin_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(ErrorResponse::new(ErrorCode::AdminKeyNotFound).with_param("id", id));
    }

    crate::db::delete_admin_key(&id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(MessageCode::AdminKeyDeleted)).into_response())
}

#[cfg(test)]
mod tests {
    use crate::err::ErrorCode;

    use super::{
        CreateAdminKeyRequest, Permission, admin_key_permissions, canonical_permission_strings,
        validate_admin_key_permissions,
    };

    #[test]
    fn validates_admin_key_resource_permissions() {
        let permissions = vec!["users:read".to_string(), "organizations:read".to_string()];

        assert!(validate_admin_key_permissions(&permissions).is_ok());
    }

    #[test]
    fn rejects_empty_admin_key_permissions() {
        let error = validate_admin_key_permissions(&[]).unwrap_err();
        assert_eq!(error.code, ErrorCode::AdminKeyPermissionsRequired);
    }

    #[test]
    fn serializes_admin_key_permissions_to_canonical_strings() {
        let permissions = vec![Permission::ApiKeysRead, Permission::ServiceAccountsCreate];

        assert_eq!(
            canonical_permission_strings(permissions),
            vec![
                "api_keys:read".to_string(),
                "service_accounts:create".to_string()
            ]
        );
    }

    #[test]
    fn rejects_super_admin_admin_key_permission() {
        let payload: CreateAdminKeyRequest = serde_json::from_value(serde_json::json!({
            "permissions": ["users:read", "super_admin"]
        }))
        .unwrap();

        let error = validate_admin_key_permissions(&payload.permissions).unwrap_err();
        assert_eq!(error.code, ErrorCode::AdminKeyPermissionInvalid);
    }

    #[test]
    fn rejects_legacy_permissions_for_new_admin_keys() {
        let payload: CreateAdminKeyRequest = serde_json::from_value(serde_json::json!({
            "permissions": ["users"]
        }))
        .unwrap();

        let error = validate_admin_key_permissions(&payload.permissions).unwrap_err();
        assert_eq!(error.code, ErrorCode::AdminKeyPermissionInvalid);
    }

    #[test]
    fn admin_key_permissions_are_deduplicated_in_catalog_order() {
        let permissions = validate_admin_key_permissions(&[
            "users:create".to_string(),
            "users:read".to_string(),
            "users:create".to_string(),
        ])
        .unwrap();

        assert_eq!(
            permissions,
            vec![Permission::UsersRead, Permission::UsersCreate]
        );
    }

    #[test]
    fn legacy_stored_permissions_are_returned_as_canonical_effective_permissions() {
        let key = db::ent::AdminKey::new("hash".to_string(), None, vec!["users".to_string()]);
        let permissions = admin_key_permissions(&key);

        assert!(permissions.contains(&Permission::UsersRead));
        assert!(permissions.contains(&Permission::UsersDelete));
        assert!(permissions.contains(&Permission::MembershipsCreate));
        assert!(!permissions.contains(&Permission::OrganizationsRead));
    }
}
