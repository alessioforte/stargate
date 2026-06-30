use super::{SUPER_ADMIN, extract_json, extract_path, extract_query};
use crate::err::{ErrorResponse, HttpError};
use crate::etc::msg::MessageResponse;
use crate::etc::reqctx::take_audit_context_from;
use crate::require_grants;
use axum::Json;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use store::Store;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;
const API_KEYS_GRANT: &str = "api_keys";

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeySchema {
    pub id: String,
    pub label: String,
    pub revoked: bool,
    pub attrs: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKeyResponse {
    id: String,
    label: String,
    attrs: Value,
    revoked: bool,
    created_at: String,
    updated_at: String,
    api_key: String,
}

#[derive(Deserialize, Debug, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListApiKeysQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
    #[serde(default)]
    pub owner_type: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub service_account_id: Option<String>,
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

#[utoipa::path(
    get,
    path = "/admin/api-keys",
    tags = ["Admin", "API Keys"],
    params(ListApiKeysQuery),
    responses(
        (status = 200, description = "List of API keys retrieved successfully", body = PaginatedResponse<ApiKeySchema>),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_api_keys(req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, API_KEYS_GRANT);

    let query: ListApiKeysQuery = extract_query(&req)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);

    let (keys, total) = if let Some(user_id) = &query.user_id {
        let keys = crate::db::get_api_keys_by_user_id(user_id)
            .await
            .map_err(ErrorResponse::internal)?;
        let total = keys.len() as i64;
        (keys, total)
    } else if let Some(sa_id) = &query.service_account_id {
        let keys = crate::db::get_api_keys_by_service_account_id(sa_id)
            .await
            .map_err(ErrorResponse::internal)?;
        let total = keys.len() as i64;
        (keys, total)
    } else if let Some(owner_type) = &query.owner_type {
        match owner_type.as_str() {
            "user" => {
                let keys = crate::db::get_all_user_api_keys(limit, offset)
                    .await
                    .map_err(ErrorResponse::internal)?;
                let total = crate::db::count_user_api_keys()
                    .await
                    .map_err(ErrorResponse::internal)?;
                (keys, total)
            }
            "service_account" => {
                let keys = crate::db::get_all_service_account_api_keys(limit, offset)
                    .await
                    .map_err(ErrorResponse::internal)?;
                let total = crate::db::count_service_account_api_keys()
                    .await
                    .map_err(ErrorResponse::internal)?;
                (keys, total)
            }
            _ => {
                return Err(ErrorResponse::from(HttpError::BadRequest(
                    "Invalid owner_type: must be 'user' or 'service_account'".to_string(),
                )));
            }
        }
    } else {
        let keys = crate::db::get_all_api_keys(limit, offset)
            .await
            .map_err(ErrorResponse::internal)?;
        let total = crate::db::count_api_keys()
            .await
            .map_err(ErrorResponse::internal)?;
        (keys, total)
    };

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
    path = "/admin/api-keys/{id}",
    tags = ["Admin", "API Keys"],
    params(("id" = String, Path, description = "API Key ID")),
    responses(
        (status = 200, description = "API key retrieved successfully", body = ApiKeySchema),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "API key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_api_key(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, API_KEYS_GRANT);

    let id: String = extract_path(&mut req).await?;

    let key = crate::db::get_api_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::NotFound(format!(
                "API key with id '{}' not found",
                id
            )))
        })?;

    Ok(Json(key).into_response())
}

#[utoipa::path(
    post,
    path = "/admin/api-keys",
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
pub async fn create_api_key(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, API_KEYS_GRANT);

    let ctx = take_audit_context_from(req.extensions_mut());
    let payload: CreateApiKeyRequest = extract_json(req).await?;

    if payload.user_id.is_none() && payload.service_account_id.is_none() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Either user_id or service_account_id must be provided".to_string(),
        )));
    }

    let secret = pw::generate_api_key();
    let key_hash = pw::hash_api_key(&secret);

    let api_key = if let Some(user_id) = &payload.user_id {
        crate::db::create_user_api_key(
            user_id,
            &key_hash,
            &payload.label,
            payload.attrs.clone(),
            ctx,
        )
        .await
        .map_err(ErrorResponse::internal)?
    } else if let Some(sa_id) = &payload.service_account_id {
        crate::db::create_service_account_api_key(
            sa_id,
            &key_hash,
            &payload.label,
            payload.attrs.clone(),
            ctx,
        )
        .await
        .map_err(ErrorResponse::internal)?
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
        created_at: api_key.created_at.to_rfc3339(),
        updated_at: api_key.updated_at.to_rfc3339(),
        api_key: secret,
    };

    Ok((StatusCode::CREATED, Json(response)).into_response())
}

#[utoipa::path(
    delete,
    path = "/admin/api-keys/{id}",
    tags = ["Admin", "API Keys"],
    params(("id" = String, Path, description = "API Key ID")),
    responses(
        (status = 200, description = "API key deleted successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "API key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn delete_api_key(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, API_KEYS_GRANT);

    let ctx = take_audit_context_from(req.extensions_mut());
    let id: String = extract_path(&mut req).await?;

    let Some(api_key) = crate::db::get_api_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "API key with id '{}' not found",
            id
        ))));
    };

    crate::db::delete_api_key(&id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    crate::etc::store::use_store()
        .delete(&api_key.key_hash)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(
        "API key revoked successfully",
        "api_key_revoked",
    ))
    .into_response())
}

#[utoipa::path(
    put,
    path = "/admin/api-keys/{id}/revoke",
    tags = ["Admin", "API Keys"],
    params(("id" = String, Path, description = "API Key ID")),
    responses(
        (status = 200, description = "API key revoked successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "API key not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn revoke_api_key(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, API_KEYS_GRANT);

    let ctx = take_audit_context_from(req.extensions_mut());
    let id: String = extract_path(&mut req).await?;

    let api_key = crate::db::get_api_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::NotFound(format!(
                "API key with id '{}' not found",
                id
            )))
        })?;

    if api_key.revoked {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "API key is already revoked".to_string(),
        )));
    }

    crate::db::revoke_api_key(&id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    crate::etc::store::use_store()
        .delete(&api_key.key_hash)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(
        "API key revoked successfully",
        "api_key_revoked",
    ))
    .into_response())
}

#[utoipa::path(
    put,
    path = "/admin/api-keys/{id}/attrs",
    tags = ["Admin", "API Keys"],
    params(("id" = String, Path, description = "API Key ID")),
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
pub async fn update_api_key_attrs(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, API_KEYS_GRANT);

    let ctx = take_audit_context_from(req.extensions_mut());
    let id: String = extract_path(&mut req).await?;
    let payload: ApiKeyAttrsRequest = extract_json(req).await?;

    let mut existing = crate::db::get_api_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::NotFound(format!(
                "API key with id '{}' not found",
                id
            )))
        })?;

    existing.attrs = payload.attrs.clone();

    let updated = crate::db::update_api_key(existing, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(updated).into_response())
}

#[utoipa::path(
    patch,
    path = "/admin/api-keys/{id}/attrs",
    tags = ["Admin", "API Keys"],
    params(("id" = String, Path, description = "API Key ID")),
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
pub async fn patch_api_key_attrs(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, API_KEYS_GRANT);

    let ctx = take_audit_context_from(req.extensions_mut());
    let id: String = extract_path(&mut req).await?;
    let payload: ApiKeyAttrsRequest = extract_json(req).await?;

    let mut existing = crate::db::get_api_key_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::NotFound(format!(
                "API key with id '{}' not found",
                id
            )))
        })?;

    existing.attrs = match (&existing.attrs, &payload.attrs) {
        (Value::Object(existing_map), Value::Object(new_map)) => {
            let mut merged = existing_map.clone();
            for (k, v) in new_map {
                merged.insert(k.clone(), v.clone());
            }
            Value::Object(merged)
        }
        _ => payload.attrs.clone(),
    };

    let updated = crate::db::update_api_key(existing, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(updated).into_response())
}
