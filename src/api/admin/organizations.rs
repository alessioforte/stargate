use crate::err::{ErrorResponse, HttpError};
use crate::etc::msg::MessageResponse;
use actix_web::{HttpMessage, HttpRequest, HttpResponse, delete, get, post, put, web};
use actix_web_grants::protect;
use db::ent::AuditContext;
use serde::{Deserialize, Serialize};

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

/// Schema only representation of Organization for OpenAPI docs
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct Organization {
    id: i64,
    name: String,
    description: Option<String>,
    attrs: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListOrganizationsQuery {
    /// Maximum number of organizations to return (default: 20, max: 100)
    #[serde(default)]
    pub limit: Option<i64>,
    /// Number of organizations to skip (default: 0)
    #[serde(default)]
    pub offset: Option<i64>,
    /// Search query to filter organizations by name or description
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

/// Get all organizations with pagination and optional search
#[utoipa::path(
    context_path = "/admin",
    path = "/organizations",
    tags = ["Admin", "Organizations"],
    params(ListOrganizationsQuery),
    responses(
        (status = 200, description = "List of organizations retrieved successfully", body = PaginatedResponse<Organization>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[get("")]
#[protect(any("super_admin", "organizations"))]
pub async fn get_organizations(
    query: web::Query<ListOrganizationsQuery>,
) -> Result<HttpResponse, ErrorResponse> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT).max(1);
    let offset = query.offset.unwrap_or(0).max(0);

    let (organizations, total) = match &query.q {
        Some(search_query) if !search_query.trim().is_empty() => {
            let organizations = crate::db::search_organizations(search_query, limit, offset)
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            let total = crate::db::count_search_organizations(search_query)
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            (organizations, total)
        }
        _ => {
            let organizations = crate::db::get_all_organizations(limit, offset)
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            let total = crate::db::count_organizations()
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            (organizations, total)
        }
    };

    let response = PaginatedResponse {
        data: organizations,
        total,
        limit,
        offset,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Get an organization by ID
#[utoipa::path(
    context_path = "/admin",
    path = "/organizations/{id}",
    tags = ["Admin", "Organizations"],
    params(
        ("id" = String, Path, description = "Organization ID")
    ),
    responses(
        (status = 200, description = "Organization retrieved successfully", body = Organization),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Organization not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[get("/{id}")]
#[protect(any("super_admin", "organizations"))]
pub async fn get_organization(params: web::Path<String>) -> Result<HttpResponse, ErrorResponse> {
    let id = params.into_inner();

    let organization = match crate::db::get_organization_by_id(&id).await {
        Ok(Some(org)) => org,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "Organization with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(organization))
}

/// Create a new organization
#[utoipa::path(
    context_path = "/admin",
    path = "/organizations",
    tags = ["Admin", "Organizations"],
    request_body = CreateOrganizationRequest,
    responses(
        (status = 201, description = "Organization created successfully", body = Organization),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[post("")]
#[protect(any("super_admin", "organizations"))]
pub async fn create_organization(
    req: HttpRequest,
    payload: web::Json<CreateOrganizationRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = req
        .extensions_mut()
        .remove::<AuditContext>()
        .unwrap_or_else(AuditContext::anonymous);

    if payload.name.trim().is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Name cannot be empty".to_string(),
        )));
    }

    let organization = match crate::db::create_organization(
        &payload.name,
        payload.description.as_deref(),
        payload.attrs.as_ref(),
        ctx,
    )
    .await
    {
        Ok(org) => org,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Created().json(organization))
}

/// Update an organization
#[utoipa::path(
    context_path = "/admin",
    path = "/organizations/{id}",
    tags = ["Admin", "Organizations"],
    params(
        ("id" = String, Path, description = "Organization ID")
    ),
    request_body = UpdateOrganizationRequest,
    responses(
        (status = 200, description = "Organization updated successfully", body = Organization),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Organization not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[put("/{id}")]
#[protect(any("super_admin", "organizations"))]
pub async fn update_organization(
    req: HttpRequest,
    params: web::Path<String>,
    payload: web::Json<UpdateOrganizationRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = req
        .extensions_mut()
        .remove::<AuditContext>()
        .unwrap_or_else(AuditContext::anonymous);

    let id = params.into_inner();

    if payload.name.trim().is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Name cannot be empty".to_string(),
        )));
    }

    // Check if organization exists
    if let Ok(None) = crate::db::get_organization_by_id(&id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "Organization with id '{}' not found",
            id
        ))));
    }

    let organization = match crate::db::update_organization(
        &id,
        &payload.name,
        payload.description.as_deref(),
        payload.attrs.as_ref(),
        ctx,
    )
    .await
    {
        Ok(org) => org,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(organization))
}

/// Delete an organization
#[utoipa::path(
    context_path = "/admin",
    path = "/organizations/{id}",
    tags = ["Admin", "Organizations"],
    params(
        ("id" = String, Path, description = "Organization ID")
    ),
    responses(
        (status = 204, description = "Organization deleted successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Organization not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[delete("/{id}")]
#[protect(any("super_admin", "organizations"))]
pub async fn delete_organization(
    req: HttpRequest,
    params: web::Path<String>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = req
        .extensions_mut()
        .remove::<AuditContext>()
        .unwrap_or_else(AuditContext::anonymous);

    let id = params.into_inner();

    if let Ok(None) = crate::db::get_organization_by_id(&id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "Organization with id '{}' not found",
            id
        ))));
    }

    match crate::db::delete_organization(&id, ctx).await {
        Ok(()) => {}
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    }

    let message = MessageResponse::new("Organization deleted successfully", "organization_deleted");

    Ok(HttpResponse::NoContent().json(message))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/organizations")
        .service(get_organizations)
        .service(create_organization)
        .service(get_organization)
        .service(update_organization)
        .service(delete_organization)
}
