use super::{AdminPrincipal, SUPER_ADMIN, extract_path, extract_query};
use crate::err::{ErrorCode, ErrorResponse};
use crate::require_grants;
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
    #[serde(default)]
    offset: Option<usize>,
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
    email: Option<String>,
    name: Option<String>,
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
    current: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedAdminSessions {
    data: Vec<AdminSession>,
    total: usize,
    limit: usize,
    offset: usize,
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
        (given_name, family_name) => crate::fun::format_name(
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
    description = "Returns active login sessions ordered by most recent token refresh. Results can be filtered by user, organization, or OAuth client.",
    params(ListSessionsQuery),
    responses(
        (status = 200, description = "Active sessions retrieved", body = PaginatedAdminSessions),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn get_sessions(req: Request) -> Result<Json<PaginatedAdminSessions>, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN);

    let query: ListSessionsQuery = extract_query(&req)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0);
    let user_id = normalized(query.user_id);
    let organization_id = normalized(query.organization_id);
    let client_id = normalized(query.client_id);
    let current_session_id = req
        .extensions()
        .get::<AdminPrincipal>()
        .and_then(|principal| principal.claims.sid.as_deref())
        .map(str::to_string);

    let mut sessions = crate::act::sessions::list_sessions()
        .await
        .map_err(ErrorResponse::internal)?;
    sessions.retain(|session| {
        user_id
            .as_deref()
            .is_none_or(|user_id| session.user_id == user_id)
            && organization_id
                .as_deref()
                .is_none_or(|organization_id| session.org_id.as_deref() == Some(organization_id))
            && client_id.as_deref().is_none_or(|client_id| {
                session
                    .client_ids
                    .iter()
                    .any(|candidate| candidate == client_id)
            })
    });

    let total = sessions.len();
    let selected = sessions.into_iter().skip(offset).take(limit);
    let mut data = Vec::new();
    for session in selected {
        let user = crate::db::get_user_by_id(&session.user_id)
            .await
            .map_err(ErrorResponse::internal)?;
        let user = match user {
            Some(user) => AdminSessionUser {
                id: session.user_id.clone(),
                email: Some(user.email.clone()),
                name: Some(user_name(&user)),
            },
            None => AdminSessionUser {
                id: session.user_id.clone(),
                email: None,
                name: None,
            },
        };
        data.push(AdminSession {
            current: current_session_id.as_deref() == Some(session.id.as_str()),
            id: session.id,
            user,
            organization_id: session.org_id,
            authenticated_at: timestamp(session.authenticated_at_unix)?,
            last_seen_at: timestamp(session.last_seen_at_unix)?,
            expires_at: timestamp(session.expires_at_unix)?,
            client_ids: session.client_ids,
        });
    }

    Ok(Json(PaginatedAdminSessions {
        data,
        total,
        limit,
        offset,
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
    require_grants!(req, SUPER_ADMIN);

    let admin_user_id = req
        .extensions()
        .get::<AdminPrincipal>()
        .map(|principal| principal.user_id.clone());
    let session_id: String = extract_path(&mut req).await?;
    let session = crate::act::sessions::revoke_session(&session_id)
        .await
        .map_err(ErrorResponse::internal)?
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
    require_grants!(req, SUPER_ADMIN);

    let admin_user_id = req
        .extensions()
        .get::<AdminPrincipal>()
        .map(|principal| principal.user_id.clone());
    let user_id: String = extract_path(&mut req).await?;
    crate::db::get_user_by_id(&user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::new(ErrorCode::UserNotFound).with_param("id", user_id.clone())
        })?;

    let revoked_sessions = crate::act::sessions::revoke_all_sessions(&user_id)
        .await
        .map_err(ErrorResponse::internal)?;
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
