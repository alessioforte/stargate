use super::{
    authorization::{self, Permission},
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
pub struct OrganizationSchema {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub attrs: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize, Debug, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListOrganizationsQuery {
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
pub struct CreateOrganizationRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub attrs: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOrganizationRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub attrs: Option<serde_json::Value>,
}

#[utoipa::path(
    get,
    path = "/admin/organizations",
    tags = ["Admin", "Organizations"],
    params(ListOrganizationsQuery),
    responses(
        (status = 200, description = "List of organizations retrieved successfully", body = PaginatedResponse<OrganizationSchema>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_organizations(req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::OrganizationsRead)?;

    let query: ListOrganizationsQuery = extract_query(&req)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);

    let (orgs, total) = match &query.q {
        Some(q) if !q.trim().is_empty() => {
            let orgs = crate::db::search_organizations(q, limit, offset)
                .await
                .map_err(ErrorResponse::internal)?;
            let total = crate::db::count_search_organizations(q)
                .await
                .map_err(ErrorResponse::internal)?;
            (orgs, total)
        }
        _ => {
            let orgs = crate::db::get_all_organizations(limit, offset)
                .await
                .map_err(ErrorResponse::internal)?;
            let total = crate::db::count_organizations()
                .await
                .map_err(ErrorResponse::internal)?;
            (orgs, total)
        }
    };

    Ok(Json(PaginatedResponse {
        data: orgs,
        total,
        limit,
        offset,
    })
    .into_response())
}

#[utoipa::path(
    get,
    path = "/admin/organizations/{id}",
    tags = ["Admin", "Organizations"],
    params(("id" = String, Path, description = "Organization ID")),
    responses(
        (status = 200, description = "Organization retrieved successfully", body = OrganizationSchema),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Organization not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_organization(mut req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::OrganizationsRead)?;

    let id: String = extract_path(&mut req).await?;

    let org = crate::db::get_organization_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::new(ErrorCode::OrganizationNotFound).with_param("id", id.clone())
        })?;

    Ok(Json(org).into_response())
}

#[utoipa::path(
    post,
    path = "/admin/organizations",
    tags = ["Admin", "Organizations"],
    request_body = CreateOrganizationRequest,
    responses(
        (status = 201, description = "Organization created successfully", body = OrganizationSchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn create_organization(mut req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::OrganizationsCreate)?;

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let payload: CreateOrganizationRequest = extract_json(req).await?;

    if payload.name.trim().is_empty() {
        return Err(ErrorResponse::new(ErrorCode::OrganizationNameRequired));
    }

    let org = crate::db::create_organization(
        &payload.name,
        payload.description.as_deref(),
        payload.attrs.as_ref(),
        ctx,
    )
    .await
    .map_err(ErrorResponse::internal)?;

    Ok((StatusCode::CREATED, Json(org)).into_response())
}

#[utoipa::path(
    put,
    path = "/admin/organizations/{id}",
    tags = ["Admin", "Organizations"],
    params(("id" = String, Path, description = "Organization ID")),
    request_body = UpdateOrganizationRequest,
    responses(
        (status = 200, description = "Organization updated successfully", body = OrganizationSchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Organization not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_organization(mut req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::OrganizationsUpdate)?;

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;
    let payload: UpdateOrganizationRequest = extract_json(req).await?;

    if payload.name.trim().is_empty() {
        return Err(ErrorResponse::new(ErrorCode::OrganizationNameRequired));
    }

    if crate::db::get_organization_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(
            ErrorResponse::new(ErrorCode::OrganizationNotFound).with_param("id", id.clone())
        );
    }

    let org = crate::db::update_organization(
        &id,
        &payload.name,
        payload.description.as_deref(),
        payload.attrs.as_ref(),
        ctx,
    )
    .await
    .map_err(ErrorResponse::internal)?;

    // Attr changes (e.g. limit overrides) must reach the gateway on the
    // next request, not after the cache TTL.
    crate::act::orgs::invalidate(&id).await;

    Ok(Json(org).into_response())
}

#[utoipa::path(
    delete,
    path = "/admin/organizations/{id}",
    tags = ["Admin", "Organizations"],
    params(("id" = String, Path, description = "Organization ID")),
    responses(
        (status = 204, description = "Organization deleted successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Organization not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_organization(mut req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::OrganizationsDelete)?;

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;

    if crate::db::get_organization_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(
            ErrorResponse::new(ErrorCode::OrganizationNotFound).with_param("id", id.clone())
        );
    }

    // Snapshot the member list first: after the delete the membership rows
    // are gone, and each member's sessions active in this org must die.
    let members = crate::db::get_organization_users(&id)
        .await
        .map_err(ErrorResponse::internal)?;

    let revoked_key_hashes = crate::db::delete_organization(&id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;
    crate::etc::guard::purge_api_key_subjects(&revoked_key_hashes).await;
    crate::act::orgs::invalidate(&id).await;

    for member in &members {
        if let Err(error) = crate::act::sessions::revoke_org_sessions(&member.user.id, &id).await {
            tracing::error!(
                user_id = %member.user.id,
                org_id = %id,
                "failed to revoke org sessions after org delete: {error}"
            );
        }
    }

    Ok((
        StatusCode::NO_CONTENT,
        Json(MessageResponse::new(MessageCode::OrganizationDeleted)),
    )
        .into_response())
}
