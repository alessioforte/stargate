use super::{
    AdminAuthenticationKind, AdminAuthenticationState, AdminPrincipal, Grants, SUPER_ADMIN,
};
use crate::err::{ErrorCode, ErrorResponse};
use crate::require_grants;
use axum::Json;
use axum::extract::Request;
use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUser {
    id: String,
    name: String,
    email: String,
    picture: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminAuthentication {
    kind: AdminAuthenticationKind,
    authenticated_at: Option<String>,
    token_issued_at: String,
    token_expires_at: String,
    client_id: Option<String>,
    session_id: Option<String>,
    scopes: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminAuthorization {
    grants: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminMe {
    user: AdminUser,
    authentication: AdminAuthentication,
    authorization: AdminAuthorization,
}

fn timestamp(value: usize) -> Result<String, ErrorResponse> {
    let seconds =
        i64::try_from(value).map_err(|_| ErrorResponse::internal("invalid token timestamp"))?;
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .map(|value| value.to_rfc3339())
        .ok_or_else(|| ErrorResponse::internal("invalid token timestamp"))
}

#[utoipa::path(
    get,
    path = "/admin/me",
    tags = ["Admin"],
    summary = "Current admin",
    description = "Returns the server-validated identity, authentication metadata, and effective grants for the current admin user.",
    responses(
        (status = 200, description = "Authenticated admin", body = AdminMe),
        (status = 401, description = "Invalid or expired token", body = ErrorResponse),
        (status = 403, description = "Authenticated principal is not a super admin", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn get_admin_me(req: Request) -> Result<Json<AdminMe>, ErrorResponse> {
    let authentication = req
        .extensions()
        .get::<AdminAuthenticationState>()
        .copied()
        .unwrap_or_default();
    if !authentication.authenticated {
        let code = if authentication.credential_present {
            ErrorCode::AuthTokenInvalid
        } else {
            ErrorCode::AuthTokenMissing
        };
        return Err(ErrorResponse::new(code));
    }
    require_grants!(req, SUPER_ADMIN);

    let principal = req
        .extensions()
        .get::<AdminPrincipal>()
        .cloned()
        .ok_or_else(|| ErrorResponse::internal("missing authenticated admin principal"))?;
    let user = crate::db::get_user_by_id(&principal.user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::internal("authenticated admin user no longer exists"))?;

    let name = match (user.given_name.as_deref(), user.family_name.as_deref()) {
        (None | Some(""), None | Some("")) => user.nickname.clone(),
        (given_name, family_name) => crate::fun::format_name(
            given_name.unwrap_or_default(),
            family_name.unwrap_or_default(),
        ),
    };
    let mut grants = req
        .extensions()
        .get::<Grants>()
        .map(|grants| grants.0.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    grants.sort_unstable();

    let claims = principal.claims;
    let scopes = claims
        .scope
        .as_deref()
        .map(|scope| scope.split_ascii_whitespace().map(str::to_string).collect())
        .unwrap_or_default();

    Ok(Json(AdminMe {
        user: AdminUser {
            id: user.id,
            name,
            email: user.email,
            picture: user.picture,
        },
        authentication: AdminAuthentication {
            kind: principal.kind,
            authenticated_at: claims.auth_time.map(timestamp).transpose()?,
            token_issued_at: timestamp(claims.iat)?,
            token_expires_at: timestamp(claims.exp)?,
            client_id: claims.azp,
            session_id: claims.sid,
            scopes,
        },
        authorization: AdminAuthorization { grants },
    }))
}

#[cfg(test)]
mod tests {
    use super::timestamp;

    #[test]
    fn timestamp_is_returned_as_rfc3339() {
        assert_eq!(timestamp(0).unwrap(), "1970-01-01T00:00:00+00:00");
    }
}
