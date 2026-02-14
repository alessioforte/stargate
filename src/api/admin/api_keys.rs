use crate::err::{ErrorResponse, HttpError};
use crate::etc::msg::MessageResponse;
use actix_web::{HttpMessage, HttpRequest, HttpResponse, delete, get, patch, post, put, web};
use actix_web_grants::protect;
use db::ent::AuditContext;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

/// Schema-only representation of ApiKey for OpenAPI docs
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct ApiKeySchema {
    id: String,
    key_hash: String,
    label: String,
    revoked: bool,
    attrs: Value,
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKeyResponse {
    id: String,
    label: String,
    attrs: Value,
    revoked: bool,
    api_key: String,
}

#[derive(Deserialize, Debug, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListApiKeysQuery {
    /// Maximum number of API keys to return (default: 20, max: 100)
    #[serde(default)]
    pub limit: Option<i64>,
    /// Number of API keys to skip (default: 0)
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
pub struct CreateApiKeyRequest {
    pub user_id: Option<String>,
    pub service_account_id: Option<String>,
    pub label: String,
    #[serde(default)]
    pub attrs: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyAttrsRequest {
    pub attrs: Value,
}

/// Get all API keys with pagination
#[utoipa::path(
    context_path = "/admin",
    path = "/api-keys",
    tags = ["Admin", "API Keys"],
    params(ListApiKeysQuery),
    responses(
        (status = 200, description = "List of API keys retrieved successfully", body = PaginatedResponse<ApiKeySchema>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[get("")]
#[protect(any("super_admin", "api_keys"))]
pub async fn get_api_keys(
    query: web::Query<ListApiKeysQuery>,
) -> Result<HttpResponse, ErrorResponse> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT).max(1);
    let offset = query.offset.unwrap_or(0).max(0);

    // TODO: Remove keyHash from the response
    let api_keys = crate::db::get_all_api_keys(limit, offset)
        .await
        .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;

    let total = crate::db::count_api_keys()
        .await
        .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;

    let response = PaginatedResponse {
        data: api_keys,
        total,
        limit,
        offset,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Get an API key by ID
#[utoipa::path(
    context_path = "/admin",
    path = "/api-keys/{id}",
    tags = ["Admin", "API Keys"],
    params(
        ("id" = String, Path, description = "API Key ID")
    ),
    responses(
        (status = 200, description = "API key retrieved successfully", body = ApiKeySchema),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "API key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[get("/{id}")]
#[protect(any("super_admin", "api_keys"))]
pub async fn get_api_key(params: web::Path<String>) -> Result<HttpResponse, ErrorResponse> {
    let id = params.into_inner();

    let api_key = match crate::db::get_api_key_by_id(&id).await {
        Ok(Some(key)) => key,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "API key with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(api_key))
}

/// Create a new API key
#[utoipa::path(
    context_path = "/admin",
    path = "/api-keys",
    tags = ["Admin", "API Keys"],
    request_body = CreateApiKeyRequest,
    responses(
        (status = 201, description = "API key created successfully", body = CreateApiKeyResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[post("")]
#[protect(any("super_admin", "api_keys"))]
pub async fn create_api_key(
    req: HttpRequest,
    payload: web::Json<CreateApiKeyRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let user_id = payload.user_id.clone();
    let service_account_id = payload.service_account_id.clone();
    if user_id.is_none() && service_account_id.is_none() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Either user_id or service_account_id must be provided".to_string(),
        )));
    }

    let secret = pw::generate_api_key();
    let key_hash = pw::hash_api_key(&secret);

    let api_key = if let Some(user_id) = user_id {
        match crate::db::create_user_api_key(
            &user_id,
            &key_hash,
            &payload.label,
            payload.attrs.clone(),
            ctx,
        )
        .await
        {
            Ok(api_key) => api_key,
            Err(e) => {
                return Err(ErrorResponse::from(HttpError::InternalServerError(
                    e.to_string(),
                )));
            }
        }
    } else if let Some(service_account_id) = service_account_id {
        match crate::db::create_service_account_api_key(
            &service_account_id,
            &key_hash,
            &payload.label,
            payload.attrs.clone(),
            ctx,
        )
        .await
        {
            Ok(api_key) => api_key,
            Err(e) => {
                return Err(ErrorResponse::from(HttpError::InternalServerError(
                    e.to_string(),
                )));
            }
        }
    } else {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Either user_id or service_account_id must be provided".to_string(),
        )));
    };

    let response = CreateApiKeyResponse {
        id: api_key.id,
        label: api_key.label,
        attrs: api_key.attrs,
        revoked: api_key.revoked,
        api_key: secret,
    };

    Ok(HttpResponse::Created().json(response))
}

/// Delete an API key
#[utoipa::path(
    context_path = "/admin",
    path = "/api-keys/{id}",
    tags = ["Admin", "API Keys"],
    params(
        ("id" = String, Path, description = "API Key ID")
    ),
    responses(
        (status = 204, description = "API key deleted successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "API key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[delete("/{id}")]
#[protect(any("super_admin", "api_keys"))]
pub async fn delete_api_key(
    req: HttpRequest,
    params: web::Path<String>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    // Check if API key exists
    if let Ok(None) = crate::db::get_api_key_by_id(&id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "API key with id '{}' not found",
            id
        ))));
    }

    match crate::db::delete_api_key(&id, ctx).await {
        Ok(()) => {}
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    }

    let message = MessageResponse::new("API key revoked successfully", "api_key_revoked");

    Ok(HttpResponse::Ok().json(message))
}

/// Revoke an API key
#[utoipa::path(
    context_path = "/admin",
    path = "/api-keys/{id}/revoke",
    tags = ["Admin", "API Keys"],
    params(
        ("id" = String, Path, description = "API Key ID")
    ),
    responses(
        (status = 200, description = "API key revoked successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "API key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[put("/{id}/revoke")]
#[protect(any("super_admin", "api_keys"))]
pub async fn revoke_api_key(
    req: HttpRequest,
    params: web::Path<String>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    // Check if API key exists
    let api_key = match crate::db::get_api_key_by_id(&id).await {
        Ok(Some(key)) => key,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "API key with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if api_key.revoked {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "API key is already revoked".to_string(),
        )));
    }

    match crate::db::revoke_api_key(&id, ctx).await {
        Ok(()) => {}
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    }

    let message = MessageResponse::new("API key revoked successfully", "api_key_revoked");

    Ok(HttpResponse::Ok().json(message))
}

/// Update API key attrs (full replacement)
#[utoipa::path(
    context_path = "/admin",
    path = "/api-keys/{id}/attrs",
    tags = ["Admin", "API Keys"],
    params(
        ("id" = String, Path, description = "API Key ID")
    ),
    request_body = ApiKeyAttrsRequest,
    responses(
        (status = 200, description = "API key attrs updated successfully", body = ApiKeySchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "API key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[put("/{id}/attrs")]
#[protect(any("super_admin", "api_keys"))]
pub async fn update_api_key_attrs(
    req: HttpRequest,
    params: web::Path<String>,
    payload: web::Json<ApiKeyAttrsRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    // Get existing API key
    let mut existing_key = match crate::db::get_api_key_by_id(&id).await {
        Ok(Some(key)) => key,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "API key with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    // Replace attrs entirely
    existing_key.attrs = payload.attrs.clone();

    let api_key = match crate::db::update_api_key(existing_key, ctx).await {
        Ok(key) => key,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(api_key))
}

/// Patch API key attrs (partial update/merge)
#[utoipa::path(
    context_path = "/admin",
    path = "/api-keys/{id}/attrs",
    tags = ["Admin", "API Keys"],
    params(
        ("id" = String, Path, description = "API Key ID")
    ),
    request_body = ApiKeyAttrsRequest,
    responses(
        (status = 200, description = "API key attrs patched successfully", body = ApiKeySchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "API key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[patch("/{id}/attrs")]
#[protect(any("super_admin", "api_keys"))]
pub async fn patch_api_key_attrs(
    req: HttpRequest,
    params: web::Path<String>,
    payload: web::Json<ApiKeyAttrsRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    // Get existing API key
    let mut existing_key = match crate::db::get_api_key_by_id(&id).await {
        Ok(Some(key)) => key,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "API key with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    // Merge attrs: if both existing and new are objects, merge them; otherwise replace
    existing_key.attrs = match (&existing_key.attrs, &payload.attrs) {
        (Value::Object(existing), Value::Object(new)) => {
            let mut merged = existing.clone();
            for (key, value) in new {
                merged.insert(key.clone(), value.clone());
            }
            Value::Object(merged)
        }
        _ => payload.attrs.clone(),
    };

    let api_key = match crate::db::update_api_key(existing_key, ctx).await {
        Ok(key) => key,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(api_key))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/api-keys")
        .service(get_api_keys)
        .service(create_api_key)
        .service(get_api_key)
        .service(delete_api_key)
        .service(revoke_api_key)
        .service(update_api_key_attrs)
        .service(patch_api_key_attrs)
}
