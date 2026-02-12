use crate::err::{ErrorResponse, HttpError};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, delete, get, patch, post, put, web};
use actix_web_grants::protect;
use db::ent::{AuditContext, CredentialType, Profile};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserSchema {
    pub id: String,
    pub email: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: String,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    pub attrs: Value,
}

/// Schema-only representation of Organization for OpenAPI docs
#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationSchema {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub attrs: Value,
}

#[derive(Deserialize, Debug, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListUsersQuery {
    /// Maximum number of users to return (default: 20, max: 100)
    #[serde(default)]
    pub limit: Option<i64>,
    /// Number of users to skip (default: 0)
    #[serde(default)]
    pub offset: Option<i64>,
    /// Search query to filter users by email, name, or nickname
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
pub struct CreateUserRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub given_name: Option<String>,
    #[serde(default)]
    pub family_name: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub picture: Option<String>,
    #[serde(default)]
    pub phone_number: Option<String>,
    #[serde(default = "default_attrs")]
    pub attrs: Value,
}

fn default_attrs() -> Value {
    Value::Null
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    pub email: String,
    #[serde(default)]
    pub given_name: Option<String>,
    #[serde(default)]
    pub family_name: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub picture: Option<String>,
    #[serde(default)]
    pub phone_number: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchUserRequest {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub given_name: Option<String>,
    #[serde(default)]
    pub family_name: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub picture: Option<String>,
    #[serde(default)]
    pub phone_number: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserAttrsRequest {
    pub attrs: Value,
}

/// Get all users with pagination and optional search
#[utoipa::path(
    context_path = "/admin",
    path = "/users",
    tags = ["Admin"],
    params(ListUsersQuery),
    responses(
        (status = 200, description = "List of users retrieved successfully", body = PaginatedResponse<UserSchema>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Internal server error")
    )
)]
#[get("")]
#[protect(any("super_admin", "users"))]
pub async fn get_users(query: web::Query<ListUsersQuery>) -> Result<HttpResponse, ErrorResponse> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT).max(1);
    let offset = query.offset.unwrap_or(0).max(0);

    let (users, total) = match &query.q {
        Some(search_query) if !search_query.trim().is_empty() => {
            println!(
                "Searching users with query: {} {} {}",
                search_query, limit, offset
            );
            let users = crate::db::search_users(search_query, limit, offset)
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            let total = crate::db::count_search_users(search_query)
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            (users, total)
        }
        _ => {
            let users = crate::db::get_all_users(limit, offset)
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            let total = crate::db::count_users()
                .await
                .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;
            (users, total)
        }
    };

    let response = PaginatedResponse {
        data: users,
        total,
        limit,
        offset,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Get a user by ID
#[utoipa::path(
    context_path = "/admin",
    path = "/users/{id}",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User retrieved successfully", body = UserSchema),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[get("/{id}")]
#[protect(any("super_admin", "users"))]
pub async fn get_user(params: web::Path<String>) -> Result<HttpResponse, ErrorResponse> {
    let id = params.into_inner();

    let user = match crate::db::get_user_by_id(&id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "User with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(user))
}

/// Create a new user
#[utoipa::path(
    context_path = "/admin",
    path = "/users",
    tags = ["Admin"],
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created successfully", body = UserSchema),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 409, description = "Conflict - user already exists"),
        (status = 500, description = "Internal server error")
    )
)]
#[post("")]
#[protect(any("super_admin", "users"))]
pub async fn create_user(
    req: HttpRequest,
    payload: web::Json<CreateUserRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    // Check if user already exists
    if let Ok(Some(_)) = crate::db::get_user_by_username(&payload.email).await {
        return Err(ErrorResponse::from(HttpError::Conflict(format!(
            "User with email '{}' already exists",
            payload.email
        ))));
    }

    // Hash the password
    let hashed_password = match pw::Hash::encode(&payload.password) {
        Ok(hash) => hash,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    // Create profile from request
    let nickname = match &payload.nickname {
        Some(n) => n.clone(),
        None => payload.email.clone(),
    };
    let profile = Profile::new(payload.email.clone(), nickname)
        .given_name(payload.given_name.clone())
        .family_name(payload.family_name.clone())
        .picture(payload.picture.clone())
        .phone_number(payload.phone_number.clone())
        .attrs(payload.attrs.clone());

    let user = match crate::db::create_user(
        profile,
        CredentialType::Password,
        &hashed_password,
        ctx,
    )
    .await
    {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Created().json(user))
}

/// Update a user (full replacement)
#[utoipa::path(
    context_path = "/admin",
    path = "/users/{id}",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "User ID")
    ),
    request_body = UpdateUserRequest,
    responses(
        (status = 200, description = "User updated successfully", body = UserSchema),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[put("/{id}")]
#[protect(any("super_admin", "users"))]
pub async fn update_user(
    req: HttpRequest,
    params: web::Path<String>,
    payload: web::Json<UpdateUserRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    // Get existing user
    let existing_user = match crate::db::get_user_by_id(&id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "User with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    // Create updated user with new values
    let updated_user = db::ent::User::new(
        payload.email.clone(),
        payload
            .nickname
            .clone()
            .unwrap_or(existing_user.nickname.clone()),
    )
    .given_name(payload.given_name.clone())
    .family_name(payload.family_name.clone())
    .picture(payload.picture.clone())
    .phone_number(payload.phone_number.clone())
    .attrs(existing_user.attrs.clone());

    // Preserve the original ID
    let mut updated_user = updated_user;
    updated_user.id = existing_user.id;

    let user = match crate::db::update_user(updated_user, ctx).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(user))
}

/// Patch a user (partial update)
#[utoipa::path(
    context_path = "/admin",
    path = "/users/{id}",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "User ID")
    ),
    request_body = PatchUserRequest,
    responses(
        (status = 200, description = "User patched successfully", body = UserSchema),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[patch("/{id}")]
#[protect(any("super_admin", "users"))]
pub async fn patch_user(
    req: HttpRequest,
    params: web::Path<String>,
    payload: web::Json<PatchUserRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    // Get existing user
    let mut existing_user = match crate::db::get_user_by_id(&id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "User with id '{}' not found",
                id
            ))));
        }
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    // Apply partial updates
    if let Some(email) = &payload.email {
        existing_user.email = email.clone();
    }
    if payload.given_name.is_some() {
        existing_user.given_name = payload.given_name.clone();
    }
    if payload.family_name.is_some() {
        existing_user.family_name = payload.family_name.clone();
    }
    if let Some(nickname) = &payload.nickname {
        existing_user.nickname = nickname.clone();
    }
    if payload.picture.is_some() {
        existing_user.picture = payload.picture.clone();
    }
    if payload.phone_number.is_some() {
        existing_user.phone_number = payload.phone_number.clone();
    }

    let user = match crate::db::update_user(existing_user, ctx).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(user))
}

/// Update user attrs (full replacement)
#[utoipa::path(
    context_path = "/admin",
    path = "/users/{id}/attrs",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "User ID")
    ),
    request_body = UserAttrsRequest,
    responses(
        (status = 200, description = "User attrs updated successfully", body = UserSchema),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[put("/{id}/attrs")]
#[protect(any("super_admin", "users"))]
pub async fn update_user_attrs(
    req: HttpRequest,
    params: web::Path<String>,
    payload: web::Json<UserAttrsRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    // Get existing user
    let mut existing_user = match crate::db::get_user_by_id(&id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "User with id '{}' not found",
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
    existing_user.attrs = payload.attrs.clone();

    let user = match crate::db::update_user(existing_user, ctx).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(user))
}

/// Patch user attrs (partial update/merge)
#[utoipa::path(
    context_path = "/admin",
    path = "/users/{id}/attrs",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "User ID")
    ),
    request_body = UserAttrsRequest,
    responses(
        (status = 200, description = "User attrs patched successfully", body = UserSchema),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[patch("/{id}/attrs")]
#[protect(any("super_admin", "users"))]
pub async fn patch_user_attrs(
    req: HttpRequest,
    params: web::Path<String>,
    payload: web::Json<UserAttrsRequest>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    // Get existing user
    let mut existing_user = match crate::db::get_user_by_id(&id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return Err(ErrorResponse::from(HttpError::NotFound(format!(
                "User with id '{}' not found",
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
    existing_user.attrs = match (&existing_user.attrs, &payload.attrs) {
        (Value::Object(existing), Value::Object(new)) => {
            let mut merged = existing.clone();
            for (key, value) in new {
                merged.insert(key.clone(), value.clone());
            }
            Value::Object(merged)
        }
        _ => payload.attrs.clone(),
    };

    let user = match crate::db::update_user(existing_user, ctx).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    Ok(HttpResponse::Ok().json(user))
}

/// Delete a user
#[utoipa::path(
    context_path = "/admin",
    path = "/users/{id}",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "User ID")
    ),
    responses(
        (status = 204, description = "User deleted successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[delete("/{id}")]
#[protect(any("super_admin", "users"))]
pub async fn delete_user(
    req: HttpRequest,
    params: web::Path<String>,
) -> Result<HttpResponse, ErrorResponse> {
    // FIXME: Prevent deletion of usper admin
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let id = params.into_inner();

    // Check if user exists
    if let Ok(None) = crate::db::get_user_by_id(&id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "User with id '{}' not found",
            id
        ))));
    }

    match crate::db::delete_user(&id, ctx).await {
        Ok(()) => {}
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    }

    Ok(HttpResponse::NoContent().finish())
}

/// Get organizations for a user
#[utoipa::path(
    context_path = "/admin",
    path = "/users/{id}/organizations",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User organizations retrieved successfully", body = Vec<OrganizationSchema>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[get("/{id}/organizations")]
#[protect(any("super_admin", "users"))]
pub async fn get_user_organizations(
    params: web::Path<String>,
) -> Result<HttpResponse, ErrorResponse> {
    let id = params.into_inner();

    // Check if user exists
    if let Ok(None) = crate::db::get_user_by_id(&id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "User with id '{}' not found",
            id
        ))));
    }

    let organizations = crate::db::get_user_organizations(&id)
        .await
        .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;

    Ok(HttpResponse::Ok().json(organizations))
}

/// Add a user to an organization
#[utoipa::path(
    context_path = "/admin",
    path = "/users/{id}/organizations/{org_id}",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "User ID"),
        ("org_id" = String, Path, description = "Organization ID")
    ),
    responses(
        (status = 204, description = "User added to organization successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "User or organization not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[put("/{id}/organizations/{org_id}")]
#[protect(any("super_admin", "users"))]
pub async fn add_user_to_organization(
    req: HttpRequest,
    params: web::Path<(String, String)>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let (user_id, org_id) = params.into_inner();

    // Check if user exists
    if let Ok(None) = crate::db::get_user_by_id(&user_id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "User with id '{}' not found",
            user_id
        ))));
    }

    // Check if organization exists
    if let Ok(None) = crate::db::get_organization_by_id(&org_id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "Organization with id '{}' not found",
            org_id
        ))));
    }

    match crate::db::add_user_to_organization(&user_id, &org_id, ctx).await {
        Ok(()) => {}
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    }

    Ok(HttpResponse::NoContent().finish())
}

/// Remove a user from an organization
#[utoipa::path(
    context_path = "/admin",
    path = "/users/{id}/organizations/{org_id}",
    tags = ["Admin"],
    params(
        ("id" = String, Path, description = "User ID"),
        ("org_id" = String, Path, description = "Organization ID")
    ),
    responses(
        (status = 204, description = "User removed from organization successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "User or organization not found"),
        (status = 500, description = "Internal server error")
    )
)]
#[delete("/{id}/organizations/{org_id}")]
#[protect(any("super_admin", "users"))]
pub async fn remove_user_from_organization(
    req: HttpRequest,
    params: web::Path<(String, String)>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };

    let (user_id, org_id) = params.into_inner();

    // Check if user exists
    if let Ok(None) = crate::db::get_user_by_id(&user_id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "User with id '{}' not found",
            user_id
        ))));
    }

    // Check if organization exists
    if let Ok(None) = crate::db::get_organization_by_id(&org_id).await {
        return Err(ErrorResponse::from(HttpError::NotFound(format!(
            "Organization with id '{}' not found",
            org_id
        ))));
    }

    match crate::db::remove_user_from_organization(&user_id, &org_id, ctx).await {
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
    web::scope("/users")
        .service(get_users)
        .service(create_user)
        .service(get_user)
        .service(update_user)
        .service(patch_user)
        .service(update_user_attrs)
        .service(patch_user_attrs)
        .service(delete_user)
        .service(get_user_organizations)
        .service(add_user_to_organization)
        .service(remove_user_from_organization)
}
