use crate::err::{ErrorResponse, HttpError};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, delete, get, post, put, web};
use actix_web_grants::protect;
use db::ent::AuditContext;
use serde::{Deserialize, Serialize};

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

/// Schema-only representation of AdminKey for OpenAPI docs
#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminKeySchema {
    pub id: String,
    pub key_hash: String,
    pub label: Option<String>,
    pub permissions: Vec<String>,
    pub revoked: bool,
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAdminKeyResponse {
    pub id: String,
    pub label: Option<String>,
    pub permissions: Vec<String>,
    pub revoked: bool,
    /// The plain API key (only returned once at creation time)
    pub api_key: String,
}

/// Simple message response
#[derive(Serialize, Debug, utoipa::ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Deserialize, Debug, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListAdminKeysQuery {
    /// Maximum number of admin keys to return (default: 20, max: 100)
    #[serde(default)]
    pub limit: Option<i64>,
    /// Number of admin keys to skip (default: 0)
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
    /// List of permissions granted to this key (e.g. ["users", "api_keys", "configurations"])
    pub permissions: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAdminKeyPermissionsRequest {
    /// Full replacement of the permissions list
    pub permissions: Vec<String>,
}

/// Get all admin keys with pagination
#[utoipa::path(
    context_path = "/admin",
    path = "/admin-keys",
    tags = ["Admin"],
    params(ListAdminKeysQuery),
    responses(
        (status = 200, description = "List of admin keys retrieved successfully", body = PaginatedResponse<AdminKeySchema>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Internal server error")
    )
)]
#[get("")]
#[protect("super_admin")]
pub async fn get_admin_keys(
    query: web::Query<ListAdminKeysQuery>,
) -> Result<HttpResponse, ErrorResponse> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT).max(1);
    let offset = query.offset.unwrap_or(0).max(0);

    // TODO: Remove keyHash from the response
    let admin_keys = crate::db::get_all_admin_keys(limit, offset)
        .await
        .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;

    let total = crate::db::count_admin_keys()
        .await
        .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;

    let response = PaginatedResponse {
        data: admin_keys,
        total,
        limit,
        offset,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Get an admin key by ID
#[utoipa::path(
    context_path = "/admin",
    path = "/admin-keys/{id}",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "Admin Key ID")
    ),
    responses(
        (status = 200, description = "Admin key retrieved successfully", body = AdminKeySchema),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Admin key not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[get("/{id}")]
#[protect("super_admin")]
pub async fn get_admin_key(params: web::Path<String>) -> Result<HttpResponse, ErrorResponse> {
    let id = params.into_inner();

    let admin_key = match crate::db::get_admin_key_by_id(&id).await {
        Ok(Some(key)) => key,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "Admin key with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(admin_key))
}

/// Create a new admin key
#[utoipa::path(
    context_path = "/admin",
    path = "/admin-keys",
    tags = ["Admin"],
    request_body = CreateAdminKeyRequest,
    responses(
        (status = 201, description = "Admin key created successfully", body = CreateAdminKeyResponse),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Internal server error")
    )
)]
#[post("")]
#[protect("super_admin")]
pub async fn create_admin_key(
    req: HttpRequest,
    payload: web::Json<CreateAdminKeyRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    if payload.permissions.is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Permissions list cannot be empty".to_string(),
        )));
    }

    let secret = pw::generate_api_key();
    let key_hash = pw::hash_api_key(&secret);

    let admin_key = match crate::db::create_admin_key(
        &key_hash,
        payload.label.clone(),
        payload.permissions.clone(),
        ctx,
    )
    .await
    {
        Ok(key) => key,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    let response = CreateAdminKeyResponse {
        id: admin_key.id,
        label: admin_key.label,
        permissions: admin_key.permissions.0,
        revoked: admin_key.revoked,
        api_key: secret, // Return the plain API key only once
    };

    Ok(HttpResponse::Created().json(response))
}

/// Update admin key permissions (full replacement)
#[utoipa::path(
    context_path = "/admin",
    path = "/admin-keys/{id}/permissions",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "Admin Key ID")
    ),
    request_body = UpdateAdminKeyPermissionsRequest,
    responses(
        (status = 200, description = "Admin key permissions updated successfully", body = AdminKeySchema),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Admin key not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[put("/{id}/permissions")]
#[protect("super_admin")]
pub async fn update_admin_key_permissions(
    req: HttpRequest,
    params: web::Path<String>,
    payload: web::Json<UpdateAdminKeyPermissionsRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    if payload.permissions.is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Permissions list cannot be empty".to_string(),
        )));
    }

    let mut existing_key = match crate::db::get_admin_key_by_id(&id).await {
        Ok(Some(key)) => key,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "Admin key with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if existing_key.revoked {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Cannot update a revoked admin key".to_string(),
        )));
    }

    existing_key.set_permissions(payload.permissions.clone());

    let admin_key = match crate::db::update_admin_key(existing_key, ctx).await {
        Ok(key) => key,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(admin_key))
}

/// Revoke an admin key
#[utoipa::path(
    context_path = "/admin",
    path = "/admin-keys/{id}/revoke",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "Admin Key ID")
    ),
    responses(
        (status = 200, description = "Admin key revoked successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Admin key not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[put("/{id}/revoke")]
#[protect("super_admin")]
pub async fn revoke_admin_key(
    req: HttpRequest,
    params: web::Path<String>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    let admin_key = match crate::db::get_admin_key_by_id(&id).await {
        Ok(Some(key)) => key,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "Admin key with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if admin_key.revoked {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Admin key is already revoked".to_string(),
        )));
    }

    match crate::db::revoke_admin_key(&id, ctx).await {
        Ok(()) => {}
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "Admin key revoked successfully"
    })))
}

/// Delete an admin key
#[utoipa::path(
    context_path = "/admin",
    path = "/admin-keys/{id}",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "Admin Key ID")
    ),
    responses(
        (status = 204, description = "Admin key deleted successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Admin key not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[delete("/{id}")]
#[protect("super_admin")]
pub async fn delete_admin_key(
    req: HttpRequest,
    params: web::Path<String>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    if let Ok(None) = crate::db::get_admin_key_by_id(&id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "Admin key with id '{}' not found",
            id
        ))));
    }

    match crate::db::delete_admin_key(&id, ctx).await {
        Ok(()) => {}
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    }

    Ok(HttpResponse::NoContent().finish())
}

pub fn routes() -> actix_web::Scope {
    web::scope("/admin-keys")
        .service(get_admin_keys)
        .service(create_admin_key)
        .service(get_admin_key)
        .service(update_admin_key_permissions)
        .service(revoke_admin_key)
        .service(delete_admin_key)
}
