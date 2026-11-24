use super::{
    AdminPrincipal,
    authorization::{self, Permission},
    extract_path, extract_query,
};
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::sess::listing::{self, SessionCursor, SessionFilter};
use axum::Json;
use axum::extract::Request;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListSessionsQuery {
    #[serde(default)]
    limit: Option<usize>,
    /// `nextCursor` of the previous page, with the same filters.
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    organization_id: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminSessionUser {
    id: String,
    email: String,
    name: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminSession {
    id: String,
    user: AdminSessionUser,
    organization_id: Option<String>,
    authenticated_at: String,
    last_seen_at: String,
    expires_at: String,
    client_ids: Vec<String>,
    /// How the user authenticated (OIDC `amr`: `pwd`, `otp`, `mfa`, `fed`).
    authentication_methods: Vec<String>,
    current: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminSessionPage {
    data: Vec<AdminSession>,
    limit: usize,
    /// Pass as `cursor` for the next page; `null` after the last one.
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionRevocationResponse {
    session_id: String,
    user_id: String,
    revoked: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserSessionsRevocationResponse {
    user_id: String,
    revoked_sessions: usize,
}

fn timestamp(value: u64) -> Result<String, ErrorResponse> {
    let seconds =
        i64::try_from(value).map_err(|_| ErrorResponse::internal("invalid session timestamp"))?;
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .map(|value| value.to_rfc3339())
        .ok_or_else(|| ErrorResponse::internal("invalid session timestamp"))
}

fn normalized(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn user_name(user: &db::ent::User) -> String {
    match (user.given_name.as_deref(), user.family_name.as_deref()) {
        (None | Some(""), None | Some("")) => user.nickname.clone(),
        (given_name, family_name) => crate::iam::user::profile::format_name(
            given_name.unwrap_or_default(),
            family_name.unwrap_or_default(),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/admin/sessions",
    tags = ["Admin", "Sessions"],
    summary = "List active sessions",
    description = "Returns one page of active login sessions, grouped by user from the newest user and newest session first. Results can be filtered by user, organization, or OAuth client. A page examines a bounded number of users, so it can hold fewer than `limit` sessions while `nextCursor` is set.",
    params(ListSessionsQuery),
    responses(
        (status = 503, description = "Security state unavailable (`auth.state_unavailable`); retry after 10 seconds", body = ErrorResponse, headers(("Retry-After" = String, description = "10 seconds for auth.state_unavailable"))),
        (status = 200, description = "Active sessions retrieved", body = AdminSessionPage),
        (status = 400, description = "Invalid query or cursor", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn get_sessions(req: Request) -> Result<Json<AdminSessionPage>, ErrorResponse> {
    authorization::require(&req, Permission::SessionsRead)?;

    let query: ListSessionsQuery = extract_query(&req)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let cursor = normalized(query.cursor)
        .map(|value| {
            SessionCursor::decode(&value).ok_or_else(|| {
                ErrorResponse::new(ErrorCode::RequestInvalidQuery)
                    .with_message("Invalid session cursor")
            })
        })
        .transpose()?;
    let user_id = normalized(query.user_id);
    let organization_id = normalized(query.organization_id);
    let client_id = normalized(query.client_id);
    let current_session_id = req
        .extensions()
        .get::<AdminPrincipal>()
        .and_then(AdminPrincipal::claims)
        .and_then(|claims| claims.sid.as_deref())
        .map(str::to_string);

    let filter = SessionFilter {
        user_id: user_id.as_deref(),
        organization_id: organization_id.as_deref(),
        client_id: client_id.as_deref(),
    };
    let page = listing::list_sessions(&filter, cursor.as_ref(), limit).await?;
    let data = page
        .sessions
        .into_iter()
        .map(|listing::ListedSession { user, record }| {
            Ok(AdminSession {
                current: current_session_id.as_deref() == Some(record.id.as_str()),
                user: AdminSessionUser {
                    name: user_name(&user),
                    id: user.id,
                    email: user.email,
                },
                id: record.id,
                organization_id: record.org_id,
                authenticated_at: timestamp(record.authenticated_at_unix)?,
                last_seen_at: timestamp(record.last_seen_at_unix)?,
                expires_at: timestamp(record.expires_at_unix)?,
                client_ids: record.client_ids,
                authentication_methods: record.amr,
            })
        })
        .collect::<Result<_, ErrorResponse>>()?;

    Ok(Json(AdminSessionPage {
        data,
        limit,
        next_cursor: page.next.as_ref().map(SessionCursor::encode),
    }))
}

#[utoipa::path(
    delete,
    path = "/admin/sessions/{session_id}",
    tags = ["Admin", "Sessions"],
    summary = "Revoke one session",
    description = "Immediately revokes one stable login session, including native refresh and Stargate-validated user OAuth tokens linked to it.",
    params(("session_id" = String, Path, description = "Stable session ID")),
    responses(
        (status = 503, description = "Security state unavailable (`auth.state_unavailable`); retry after 10 seconds", body = ErrorResponse, headers(("Retry-After" = String, description = "10 seconds for auth.state_unavailable"))),
        (status = 200, description = "Session revoked", body = SessionRevocationResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn delete_session(
    mut req: Request,
) -> Result<Json<SessionRevocationResponse>, ErrorResponse> {
    authorization::require(&req, Permission::SessionsRevoke)?;

    let admin_user_id = req
        .extensions()
        .get::<AdminPrincipal>()
        .and_then(AdminPrincipal::user_id)
        .map(str::to_string);
    let session_id: String = extract_path(&mut req).await?;
    let target = crate::iam::sess::registry::get_session(&session_id)
        .await?
        .ok_or_else(|| {
            ErrorResponse::new(ErrorCode::SessionNotFound).with_param("id", session_id.clone())
        })?;
    authorization::get(&req)?
        .require_user_mutation(&target.user_id)
        .await?;
    let session = crate::iam::sess::registry::revoke_session(&session_id)
        .await?
        .ok_or_else(|| {
            ErrorResponse::new(ErrorCode::SessionNotFound).with_param("id", session_id.clone())
        })?;

    tracing::info!(
        admin_user_id,
        session_id = %session.id,
        user_id = %session.user_id,
        "admin revoked login session"
    );
    Ok(Json(SessionRevocationResponse {
        session_id: session.id,
        user_id: session.user_id,
        revoked: true,
    }))
}

#[utoipa::path(
    delete,
    path = "/admin/users/{user_id}/sessions",
    tags = ["Admin", "Sessions"],
    summary = "Revoke all user sessions",
    description = "Immediately revokes every active login session belonging to one user.",
    params(("user_id" = String, Path, description = "User ID")),
    responses(
        (status = 503, description = "Security state unavailable (`auth.state_unavailable`); retry after 10 seconds", body = ErrorResponse, headers(("Retry-After" = String, description = "10 seconds for auth.state_unavailable"))),
        (status = 200, description = "User sessions revoked", body = UserSessionsRevocationResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn delete_user_sessions(
    mut req: Request,
) -> Result<Json<UserSessionsRevocationResponse>, ErrorResponse> {
    authorization::require(&req, Permission::SessionsRevoke)?;

    let admin_user_id = req
        .extensions()
        .get::<AdminPrincipal>()
        .and_then(AdminPrincipal::user_id)
        .map(str::to_string);
    let user_id: String = extract_path(&mut req).await?;
    authorization::get(&req)?
        .require_user_mutation(&user_id)
        .await?;
    crate::data::db::get_user_by_id(&user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::new(ErrorCode::UserNotFound).with_param("id", user_id.clone())
        })?;

    let revoked_sessions = crate::iam::sess::registry::revoke_all_sessions(&user_id).await?;
    tracing::info!(
        admin_user_id,
        user_id = %user_id,
        revoked_sessions,
        "admin revoked all user login sessions"
    );
    Ok(Json(UserSessionsRevocationResponse {
        user_id,
        revoked_sessions,
    }))
}

#[cfg(test)]
mod tests {
    use super::{normalized, timestamp};

    #[test]
    fn query_strings_are_trimmed() {
        assert_eq!(
            normalized(Some(" user ".to_string())).as_deref(),
            Some("user")
        );
        assert_eq!(normalized(Some("  ".to_string())), None);
    }

    #[test]
    fn session_timestamp_is_rfc3339() {
        assert_eq!(timestamp(0).unwrap(), "1970-01-01T00:00:00+00:00");
    }
}
