use crate::act::sessions::authenticated_session;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::ext::RequestExt;
use axum::Json;
use axum::extract::{FromRequest, Request};
use axum::response::Response;
use serde::{Deserialize, Serialize};

/// One of the caller's org memberships.
#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountOrganizationSchema {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub role: String,
    pub member_since: Option<String>,
    /// Whether this org is the active context of the current session.
    pub active: bool,
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountOrganizationsResponse {
    /// The org the current session acts in, if any.
    pub active_org_id: Option<String>,
    pub organizations: Vec<AccountOrganizationSchema>,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SwitchOrganizationRequest {
    pub org_id: String,
}

#[utoipa::path(
    get,
    path = "/account/organizations",
    tags = ["Account"],
    summary = "List My Organizations",
    description = "List the organizations the authenticated user belongs to, with the membership role and which one the current session acts in.",
    responses(
        (status = 200, description = "OK", body = AccountOrganizationsResponse),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn get_account_organizations(req: Request) -> Result<Response, ErrorResponse> {
    let session = authenticated_session(req.get_token()).await?;
    let active_org_id = session.subject.org_id.clone();

    let memberships = crate::db::get_user_organizations(&session.user.id)
        .await
        .map_err(ErrorResponse::internal)?;

    let organizations = memberships
        .into_iter()
        .map(|membership| AccountOrganizationSchema {
            active: active_org_id.as_deref() == Some(membership.organization.id.as_str()),
            id: membership.organization.id,
            name: membership.organization.name,
            description: membership.organization.description,
            role: membership.role,
            member_since: membership.member_since.map(|at| at.to_rfc3339()),
        })
        .collect();

    Ok(axum::response::IntoResponse::into_response(Json(
        AccountOrganizationsResponse {
            active_org_id,
            organizations,
        },
    )))
}

#[utoipa::path(
    put,
    path = "/account/session/organization",
    tags = ["Account"],
    summary = "Switch Active Organization",
    description = "Issue a fresh token pair whose session acts in the requested organization. The current tokens keep working with their original org context until they expire (fork semantics), so clients can hold parallel org contexts.",
    request_body = SwitchOrganizationRequest,
    responses(
        (status = 200, description = "OK", body = crate::api::account::AuthResponse),
        (status = 400, description = "Bad Request - not a member of the organization", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn put_session_organization(req: Request) -> Result<Response, ErrorResponse> {
    let token = req.get_token();
    let Json(body) = Json::<SwitchOrganizationRequest>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;

    let session = authenticated_session(token).await?;

    // Fork: the old sid stays alive until its TTL, so previously issued
    // tokens keep their org context; membership is validated inside the
    // session issue via the requested-org path.
    let auth_time = session
        .auth_time
        .unwrap_or_else(|| chrono::Utc::now().timestamp() as usize);

    super::session::issue_user_session(session.user, auth_time, Some(&body.org_id)).await
}
