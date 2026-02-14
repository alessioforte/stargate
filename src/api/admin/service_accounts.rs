use crate::err::{ErrorResponse, HttpError};
use crate::etc::msg::MessageResponse;
use actix_web::{HttpMessage, HttpRequest, HttpResponse, delete, get, post, put, web};
use actix_web_grants::protect;
use db::ent::AuditContext;
use serde::{Deserialize, Serialize};

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

/// Schema only representation of ServiceAccount for OpenAPI docs
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct ServiceAccountSchema {
    id: String,
    name: String,
    description: Option<String>,
    org_id: Option<String>,
}

#[derive(Deserialize, Debug, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListServiceAccountsQuery {
    /// Maximum number of service accounts to return (default: 20, max: 100)
    #[serde(default)]
    pub limit: Option<i64>,
    /// Number of service accounts to skip (default: 0)
    #[serde(default)]
    pub offset: Option<i64>,
    /// Search query to filter service accounts by name or description
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

/// Get all service accounts with pagination and optional search
#[utoipa::path(
    context_path = "/admin",
    path = "/service-accounts",
    tags = ["Admin", "Service Accounts"],
    params(ListServiceAccountsQuery),
    responses(
        (status = 200, description = "List of service accounts retrieved successfully", body = PaginatedResponse<ServiceAccountSchema>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[get("")]
#[protect(any("super_admin", "service_accounts"))]
pub async fn get_service_accounts(
    query: web::Query<ListServiceAccountsQuery>,
) -> Result<HttpResponse, ErrorResponse> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT).max(1);
    let offset = query.offset.unwrap_or(0).max(0);

    let (accounts, total) = match &query.q {
        Some(search_query) if !search_query.trim().is_empty() => {
            let accounts = crate::db::search_service_accounts(search_query, limit, offset)
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            let total = crate::db::count_search_service_accounts(search_query)
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            (accounts, total)
        }
        _ => {
            let accounts = crate::db::get_all_service_accounts(limit, offset)
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            let total = crate::db::count_service_accounts()
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            (accounts, total)
        }
    };

    let response = PaginatedResponse {
        data: accounts,
        total,
        limit,
        offset,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Get a service account by ID
#[utoipa::path(
    context_path = "/admin",
    path = "/service-accounts/{id}",
    tags = ["Admin", "Service Accounts"],
    params(
        ("id" = String, Path, description = "Service Account ID")
    ),
    responses(
        (status = 200, description = "Service account retrieved successfully", body = ServiceAccountSchema),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Service account not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[get("/{id}")]
#[protect(any("super_admin", "service_accounts"))]
pub async fn get_service_account(params: web::Path<String>) -> Result<HttpResponse, ErrorResponse> {
    let id = params.into_inner();

    let account = match crate::db::get_service_account_by_id(&id).await {
        Ok(Some(account)) => account,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "Service account with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(account))
}

/// Create a new service account
#[utoipa::path(
    context_path = "/admin",
    path = "/service-accounts",
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
#[post("")]
#[protect(any("super_admin", "service_accounts"))]
pub async fn create_service_account(
    req: HttpRequest,
    payload: web::Json<CreateServiceAccountRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    if payload.name.trim().is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Name cannot be empty".to_string(),
        )));
    }

    let account = match crate::db::create_service_account(
        &payload.name,
        payload.description.as_deref(),
        payload.org_id.as_deref(),
        ctx,
    )
    .await
    {
        Ok(account) => account,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Created().json(account))
}

/// Update a service account
#[utoipa::path(
    context_path = "/admin",
    path = "/service-accounts/{id}",
    tags = ["Admin", "Service Accounts"],
    params(
        ("id" = String, Path, description = "Service Account ID")
    ),
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
#[put("/{id}")]
#[protect(any("super_admin", "service_accounts"))]
pub async fn update_service_account(
    req: HttpRequest,
    params: web::Path<String>,
    payload: web::Json<UpdateServiceAccountRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    if payload.name.trim().is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "Name cannot be empty".to_string(),
        )));
    }

    // Check if service account exists
    if let Ok(None) = crate::db::get_service_account_by_id(&id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "Service account with id '{}' not found",
            id
        ))));
    }

    let account = match crate::db::update_service_account(
        &id,
        &payload.name,
        payload.description.as_deref(),
        ctx,
    )
    .await
    {
        Ok(account) => account,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(account))
}

/// Delete a service account
#[utoipa::path(
    context_path = "/admin",
    path = "/service-accounts/{id}",
    tags = ["Admin", "Service Accounts"],
    params(
        ("id" = String, Path, description = "Service Account ID")
    ),
    responses(
        (status = 204, description = "Service account deleted successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Service account not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[delete("/{id}")]
#[protect(any("super_admin", "service_accounts"))]
pub async fn delete_service_account(
    req: HttpRequest,
    params: web::Path<String>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    if let Ok(None) = crate::db::get_service_account_by_id(&id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "Service account with id '{}' not found",
            id
        ))));
    }

    match crate::db::delete_service_account(&id, ctx).await {
        Ok(()) => {}
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    }

    let message = MessageResponse::new(
        "Service account deleted successfully",
        "service_account_deleted",
    );

    Ok(HttpResponse::Ok().json(message))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/service-accounts")
        .service(get_service_accounts)
        .service(create_service_account)
        .service(get_service_account)
        .service(update_service_account)
        .service(delete_service_account)
}
