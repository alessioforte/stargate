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

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;
const SA_GRANT: &str = "service_accounts";

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountSchema {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub org_id: Option<String>,
}

#[derive(Deserialize, Debug, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListServiceAccountsQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
    #[serde(default)]
    pub q: Option<String>,
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
pub struct CreateServiceAccountRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub org_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateServiceAccountRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[utoipa::path(
    get,
    path = "/admin/service-accounts",
    tags = ["Admin", "Service Accounts"],
    params(ListServiceAccountsQuery),
    responses(
        (status = 200, description = "List of service accounts retrieved successfully", body = PaginatedResponse<ServiceAccountSchema>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_service_accounts(req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, SA_GRANT);

    let query: ListServiceAccountsQuery = extract_query(&req)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);

    let (accounts, total) = match &query.q {
        Some(q) if !q.trim().is_empty() => {
            let accounts = crate::db::search_service_accounts(q, limit, offset)
                .await
                .map_err(ErrorResponse::internal)?;
            let total = crate::db::count_search_service_accounts(q)
                .await
                .map_err(ErrorResponse::internal)?;
            (accounts, total)
        }
        _ => {
            let accounts = crate::db::get_all_service_accounts(limit, offset)
                .await
                .map_err(ErrorResponse::internal)?;
            let total = crate::db::count_service_accounts()
                .await
                .map_err(ErrorResponse::internal)?;
            (accounts, total)
        }
    };

    Ok(Json(PaginatedResponse {
        data: accounts,
        total,
        limit,
        offset,
    })
    .into_response())
}

#[utoipa::path(
    get,
    path = "/admin/service-accounts/{id}",
    tags = ["Admin", "Service Accounts"],
    params(("id" = String, Path, description = "Service Account ID")),
    responses(
        (status = 200, description = "Service account retrieved successfully", body = ServiceAccountSchema),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Service account not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_service_account(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, SA_GRANT);

    let id: String = extract_path(&mut req).await?;

    let account = crate::db::get_service_account_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::NotFound(format!(
                "Service account with id '{}' not found",
                id
            )))
        })?;

    Ok(Json(account).into_response())
}

#[utoipa::path(
    post,
    path = "/admin/service-accounts",
    tags = ["Admin", "Service Accounts"],
    request_body = CreateServiceAccountRequest,
    responses(
        (status = 201, description = "Service account created successfully", body = ServiceAccountSchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn create_service_account(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, SA_GRANT);

    let ctx = take_audit_context_from(req.extensions_mut());
    let payload: CreateServiceAccountRequest = extract_json(req).await?;

    if payload.name.trim().is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Name cannot be empty".to_string(),
        )));
    }

    let account = crate::db::create_service_account(
        &payload.name,
        payload.description.as_deref(),
        payload.org_id.as_deref(),
        ctx,
    )
    .await
    .map_err(ErrorResponse::internal)?;

    Ok((StatusCode::CREATED, Json(account)).into_response())
}

#[utoipa::path(
    put,
    path = "/admin/service-accounts/{id}",
    tags = ["Admin", "Service Accounts"],
    params(("id" = String, Path, description = "Service Account ID")),
    request_body = UpdateServiceAccountRequest,
    responses(
        (status = 200, description = "Service account updated successfully", body = ServiceAccountSchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Service account not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_service_account(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, SA_GRANT);

    let ctx = take_audit_context_from(req.extensions_mut());
    let id: String = extract_path(&mut req).await?;
    let payload: UpdateServiceAccountRequest = extract_json(req).await?;

    if payload.name.trim().is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Name cannot be empty".to_string(),
        )));
    }

    if crate::db::get_service_account_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "Service account with id '{}' not found",
            id
        ))));
    }

    let account =
        crate::db::update_service_account(&id, &payload.name, payload.description.as_deref(), ctx)
            .await
            .map_err(ErrorResponse::internal)?;

    Ok(Json(account).into_response())
}

#[utoipa::path(
    delete,
    path = "/admin/service-accounts/{id}",
    tags = ["Admin", "Service Accounts"],
    params(("id" = String, Path, description = "Service Account ID")),
    responses(
        (status = 200, description = "Service account deleted successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Service account not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn delete_service_account(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, SA_GRANT);

    let ctx = take_audit_context_from(req.extensions_mut());
    let id: String = extract_path(&mut req).await?;

    if crate::db::get_service_account_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "Service account with id '{}' not found",
            id
        ))));
    }

    crate::db::delete_service_account(&id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(
        "Service account deleted successfully",
        "service_account_deleted",
    ))
    .into_response())
}
