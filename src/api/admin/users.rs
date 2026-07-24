use super::{SUPER_ADMIN, extract_json, extract_optional_json, extract_path, extract_query};
use crate::act::PendingSignupProfile;
use crate::api::admin::take_admin_audit_context;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::msg::{MessageCode, MessageResponse};
use crate::require_grants;
use axum::Json;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use db::ent::{CredentialType, Profile};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;
const USERS_GRANT: &str = "users";

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
    pub created_at: String,
    pub updated_at: String,
}

/// An organization a user belongs to, with the membership role.
#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserOrganizationSchema {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub attrs: Value,
    pub created_at: String,
    pub updated_at: String,
    pub role: String,
    pub member_since: Option<String>,
}

/// A member of an organization, with the membership role.
#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationUserSchema {
    pub id: String,
    pub email: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: String,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    pub attrs: Value,
    pub created_at: String,
    pub updated_at: String,
    pub role: String,
    pub member_since: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationMembershipRequest {
    /// Membership role; free-form with the convention `owner` | `admin` |
    /// `member`. Defaults to `member` when the body is omitted.
    #[serde(default)]
    pub role: Option<String>,
}

const DEFAULT_MEMBERSHIP_ROLE: &str = "member";
const MAX_MEMBERSHIP_ROLE_LEN: usize = 50;

fn normalize_membership_role(role: Option<String>) -> Result<String, ErrorResponse> {
    let Some(role) = role else {
        return Ok(DEFAULT_MEMBERSHIP_ROLE.to_string());
    };
    let role = role.trim();
    if role.is_empty() {
        return Err(ErrorResponse::new(ErrorCode::MembershipRoleRequired));
    }
    if role.len() > MAX_MEMBERSHIP_ROLE_LEN {
        return Err(ErrorResponse::new(ErrorCode::MembershipRoleTooLong)
            .with_param("maxLength", MAX_MEMBERSHIP_ROLE_LEN));
    }
    Ok(role.to_string())
}

#[derive(Deserialize, Debug, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListUsersQuery {
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

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserInvitationRequest {
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
    #[serde(default = "default_invitation_attrs")]
    pub attrs: Value,
}

fn default_attrs() -> Value {
    Value::Null
}

fn default_invitation_attrs() -> Value {
    Value::Object(serde_json::Map::new())
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

#[utoipa::path(
    get,
    path = "/admin/users",
    tags = ["Admin", "Users"],
    params(ListUsersQuery),
    responses(
        (status = 200, description = "List of users retrieved successfully", body = PaginatedResponse<UserSchema>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_users(req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let query: ListUsersQuery = extract_query(&req)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);

    let (users, total) = match &query.q {
        Some(q) if !q.trim().is_empty() => {
            let users = crate::db::search_users(q, limit, offset)
                .await
                .map_err(ErrorResponse::internal)?;
            let total = crate::db::count_search_users(q)
                .await
                .map_err(ErrorResponse::internal)?;
            (users, total)
        }
        _ => {
            let users = crate::db::get_all_users(limit, offset)
                .await
                .map_err(ErrorResponse::internal)?;
            let total = crate::db::count_users()
                .await
                .map_err(ErrorResponse::internal)?;
            (users, total)
        }
    };

    Ok(Json(PaginatedResponse {
        data: users,
        total,
        limit,
        offset,
    })
    .into_response())
}

#[utoipa::path(
    get,
    path = "/admin/users/super-admins",
    tags = ["Admin", "Users"],
    responses(
        (status = 200, description = "Super admin users retrieved successfully", body = Vec<UserSchema>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_super_admin_users(req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let users = crate::db::get_super_admin_users()
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(users).into_response())
}

#[utoipa::path(
    get,
    path = "/admin/users/{id}",
    tags = ["Admin", "Users"],
    params(("id" = String, Path, description = "User ID")),
    responses(
        (status = 200, description = "User retrieved successfully", body = UserSchema),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_user(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let id: String = extract_path(&mut req).await?;

    let user = crate::db::get_user_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::UserNotFound).with_param("id", id.clone()))?;

    Ok(Json(user).into_response())
}

#[utoipa::path(
    post,
    path = "/admin/users",
    tags = ["Admin", "Users"],
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created successfully", body = UserSchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 409, description = "Conflict - user already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn create_user(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let payload: CreateUserRequest = extract_json(req).await?;

    if let Ok(Some(_)) = crate::db::get_user_by_username(&payload.email).await {
        return Err(ErrorResponse::new(ErrorCode::UserEmailAlreadyExists)
            .with_param("email", payload.email.clone()));
    }

    let nickname = payload
        .nickname
        .clone()
        .unwrap_or_else(|| payload.email.clone());

    crate::act::password_policy::validate_global(
        &payload.password,
        Some(&nickname),
        Some(&payload.email),
    )
    .map_err(|message| {
        ErrorResponse::new(ErrorCode::PasswordPolicyViolation).with_message(message)
    })?;

    let hashed = crate::etc::pw::hash_password(payload.password.clone())
        .await
        .ok_or_else(|| ErrorResponse::internal("failed to hash password"))?;
    let profile = Profile::new(payload.email.clone(), nickname)
        .given_name(payload.given_name.clone())
        .family_name(payload.family_name.clone())
        .picture(payload.picture.clone())
        .phone_number(payload.phone_number.clone())
        .attrs(payload.attrs.clone());

    let user = crate::db::create_user(profile, CredentialType::Password, &hashed, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok((StatusCode::CREATED, Json(user)).into_response())
}

#[utoipa::path(
    post,
    path = "/admin/users/invitations",
    tags = ["Admin", "Users"],
    request_body = CreateUserInvitationRequest,
    responses(
        (status = 202, description = "User invitation sent", body = MessageResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 409, description = "Conflict - user or invitation already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn create_user_invitation(req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let payload: CreateUserInvitationRequest = extract_json(req).await?;

    if crate::db::get_user_by_username(&payload.email)
        .await
        .map_err(ErrorResponse::internal)?
        .is_some()
    {
        return Err(ErrorResponse::new(ErrorCode::UserEmailAlreadyExists)
            .with_param("email", payload.email.clone()));
    }

    if let Some(nickname) = payload
        .nickname
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        && crate::db::get_user_by_username(nickname)
            .await
            .map_err(ErrorResponse::internal)?
            .is_some()
    {
        return Err(ErrorResponse::new(ErrorCode::UserNicknameAlreadyExists)
            .with_param("nickname", nickname));
    }

    let profile = PendingSignupProfile {
        email: payload.email,
        given_name: payload.given_name,
        family_name: payload.family_name,
        nickname: payload.nickname,
        picture: payload.picture,
        phone_number: payload.phone_number,
        attrs: payload.attrs,
    };
    crate::api::signup::request::send_signup_request(profile).await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(MessageResponse::new(MessageCode::UserInvitationSent)),
    )
        .into_response())
}

#[utoipa::path(
    put,
    path = "/admin/users/{id}",
    tags = ["Admin", "Users"],
    params(("id" = String, Path, description = "User ID")),
    request_body = UpdateUserRequest,
    responses(
        (status = 200, description = "User updated successfully", body = UserSchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_user(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;
    let payload: UpdateUserRequest = extract_json(req).await?;

    let existing = crate::db::get_user_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::UserNotFound).with_param("id", id.clone()))?;

    let mut updated = db::ent::User::new(
        payload.email.clone(),
        payload
            .nickname
            .clone()
            .unwrap_or(existing.nickname.clone()),
    )
    .given_name(payload.given_name.clone())
    .family_name(payload.family_name.clone())
    .picture(payload.picture.clone())
    .phone_number(payload.phone_number.clone())
    .attrs(existing.attrs.clone());
    updated.id = existing.id;

    let user = crate::db::update_user(updated, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(user).into_response())
}

#[utoipa::path(
    patch,
    path = "/admin/users/{id}",
    tags = ["Admin", "Users"],
    params(("id" = String, Path, description = "User ID")),
    request_body = PatchUserRequest,
    responses(
        (status = 200, description = "User patched successfully", body = UserSchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn patch_user(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;
    let payload: PatchUserRequest = extract_json(req).await?;

    let mut existing = crate::db::get_user_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::UserNotFound).with_param("id", id.clone()))?;

    if let Some(email) = &payload.email {
        existing.email = email.clone();
    }
    if payload.given_name.is_some() {
        existing.given_name = payload.given_name.clone();
    }
    if payload.family_name.is_some() {
        existing.family_name = payload.family_name.clone();
    }
    if let Some(nickname) = &payload.nickname {
        existing.nickname = nickname.clone();
    }
    if payload.picture.is_some() {
        existing.picture = payload.picture.clone();
    }
    if payload.phone_number.is_some() {
        existing.phone_number = payload.phone_number.clone();
    }

    let user = crate::db::update_user(existing, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(user).into_response())
}

#[utoipa::path(
    put,
    path = "/admin/users/{id}/attrs",
    tags = ["Admin", "Users"],
    params(("id" = String, Path, description = "User ID")),
    request_body = UserAttrsRequest,
    responses(
        (status = 200, description = "User attrs updated successfully", body = UserSchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_user_attrs(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;
    let payload: UserAttrsRequest = extract_json(req).await?;

    let mut existing = crate::db::get_user_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::UserNotFound).with_param("id", id.clone()))?;

    existing.attrs = payload.attrs.clone();

    let user = crate::db::update_user(existing, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(user).into_response())
}

#[utoipa::path(
    patch,
    path = "/admin/users/{id}/attrs",
    tags = ["Admin", "Users"],
    params(("id" = String, Path, description = "User ID")),
    request_body = UserAttrsRequest,
    responses(
        (status = 200, description = "User attrs patched successfully", body = UserSchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn patch_user_attrs(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;
    let payload: UserAttrsRequest = extract_json(req).await?;

    let mut existing = crate::db::get_user_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::UserNotFound).with_param("id", id.clone()))?;

    existing.attrs = match (&existing.attrs, &payload.attrs) {
        (Value::Object(existing_map), Value::Object(new_map)) => {
            let mut merged = existing_map.clone();
            for (key, value) in new_map {
                merged.insert(key.clone(), value.clone());
            }
            Value::Object(merged)
        }
        _ => payload.attrs.clone(),
    };

    let user = crate::db::update_user(existing, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(user).into_response())
}

#[utoipa::path(
    delete,
    path = "/admin/users/{id}",
    tags = ["Admin", "Users"],
    params(("id" = String, Path, description = "User ID")),
    responses(
        (status = 200, description = "User deleted successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn delete_user(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let id: String = extract_path(&mut req).await?;

    let user = crate::db::get_user_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::UserNotFound).with_param("id", id.clone()))?;

    let is_super = crate::db::is_super_admin_user_id(&user.id)
        .await
        .map_err(ErrorResponse::internal)?;
    if is_super {
        return Err(ErrorResponse::new(ErrorCode::UserSuperAdminDeleteForbidden));
    }

    let revoked_key_hashes = crate::db::delete_user(&id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    // Kill everything that could still authenticate as this user: cached
    // subjects of the just-revoked keys, and every live session.
    crate::etc::guard::purge_api_key_subjects(&revoked_key_hashes).await;
    if let Err(error) = crate::act::sessions::revoke_all_sessions(&id).await {
        tracing::error!(user_id = %id, "failed to revoke sessions after user delete: {error}");
    }

    Ok(Json(MessageResponse::new(MessageCode::UserDeleted).with_param("id", id)).into_response())
}

#[utoipa::path(
    get,
    path = "/admin/users/{id}/organizations",
    tags = ["Admin", "Users"],
    params(("id" = String, Path, description = "User ID")),
    responses(
        (status = 200, description = "User organizations retrieved successfully", body = Vec<UserOrganizationSchema>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_user_organizations(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let id: String = extract_path(&mut req).await?;

    let exists = crate::db::get_user_by_id(&id)
        .await
        .map_err(ErrorResponse::internal)?;
    if exists.is_none() {
        return Err(ErrorResponse::new(ErrorCode::UserNotFound).with_param("id", id.clone()));
    }

    let orgs = crate::db::get_user_organizations(&id)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(orgs).into_response())
}

#[utoipa::path(
    get,
    path = "/admin/users/organizations/{org_id}",
    tags = ["Admin", "Users"],
    params(
        ("org_id" = String, Path, description = "Organization ID"),
        ListUsersQuery
    ),
    responses(
        (status = 200, description = "Organization users retrieved successfully", body = PaginatedResponse<OrganizationUserSchema>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Organization not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_organization_users(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let query: ListUsersQuery = extract_query(&req)?;
    let org_id: String = extract_path(&mut req).await?;

    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);

    if crate::db::get_organization_by_id(&org_id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(
            ErrorResponse::new(ErrorCode::OrganizationNotFound).with_param("id", org_id.clone())
        );
    }

    let users = crate::db::get_organization_users_paginated(&org_id, limit, offset)
        .await
        .map_err(ErrorResponse::internal)?;
    let total = crate::db::count_organization_users(&org_id)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(PaginatedResponse {
        data: users,
        total,
        limit,
        offset,
    })
    .into_response())
}

#[utoipa::path(
    put,
    path = "/admin/users/{id}/organizations/{org_id}",
    tags = ["Admin", "Users"],
    params(
        ("id" = String, Path, description = "User ID"),
        ("org_id" = String, Path, description = "Organization ID")
    ),
    request_body(content = OrganizationMembershipRequest, description = "Optional membership settings; omit for the default role", content_type = "application/json"),
    responses(
        (status = 200, description = "User added to organization successfully", body = MessageResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User or organization not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn add_user_to_organization(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let (user_id, org_id): (String, String) = extract_path(&mut req).await?;
    let payload: Option<OrganizationMembershipRequest> = extract_optional_json(req).await?;
    let role = normalize_membership_role(payload.and_then(|p| p.role))?;

    if crate::db::get_user_by_id(&user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(ErrorResponse::new(ErrorCode::UserNotFound).with_param("id", user_id.clone()));
    }
    if crate::db::get_organization_by_id(&org_id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(
            ErrorResponse::new(ErrorCode::OrganizationNotFound).with_param("id", org_id.clone())
        );
    }

    crate::db::add_user_to_organization(&user_id, &org_id, &role, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(MessageCode::UserAddedToOrganization)).into_response())
}

#[utoipa::path(
    delete,
    path = "/admin/users/{id}/organizations/{org_id}",
    tags = ["Admin", "Users"],
    params(
        ("id" = String, Path, description = "User ID"),
        ("org_id" = String, Path, description = "Organization ID")
    ),
    responses(
        (status = 200, description = "User removed from organization successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User or organization not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn remove_user_from_organization(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, USERS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let (user_id, org_id): (String, String) = extract_path(&mut req).await?;

    if crate::db::get_user_by_id(&user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(ErrorResponse::new(ErrorCode::UserNotFound).with_param("id", user_id.clone()));
    }
    if crate::db::get_organization_by_id(&org_id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(
            ErrorResponse::new(ErrorCode::OrganizationNotFound).with_param("id", org_id.clone())
        );
    }

    let revoked_key_hashes = crate::db::remove_user_from_organization(&user_id, &org_id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    // The membership is gone: org-bound keys were revoked in the same
    // transaction (purge their cached subjects), and sessions acting in
    // this org die now. Org-less and other-org sessions survive.
    crate::etc::guard::purge_api_key_subjects(&revoked_key_hashes).await;
    if let Err(error) = crate::act::sessions::revoke_org_sessions(&user_id, &org_id).await {
        tracing::error!(
            user_id = %user_id,
            org_id = %org_id,
            "failed to revoke org sessions after membership removal: {error}"
        );
    }

    Ok(Json(MessageResponse::new(
        MessageCode::UserRemovedFromOrganization,
    ))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::normalize_membership_role;

    #[test]
    fn membership_role_defaults_to_member() {
        assert_eq!(normalize_membership_role(None).unwrap(), "member");
    }

    #[test]
    fn membership_role_is_trimmed() {
        assert_eq!(
            normalize_membership_role(Some("  admin  ".to_string())).unwrap(),
            "admin"
        );
    }

    #[test]
    fn membership_role_rejects_empty_and_oversized() {
        assert!(normalize_membership_role(Some("   ".to_string())).is_err());
        assert!(normalize_membership_role(Some("x".repeat(51))).is_err());
        assert!(normalize_membership_role(Some("x".repeat(50))).is_ok());
    }
}
