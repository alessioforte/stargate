//! The signed-in user's OAuth consents: list and revoke.
//!
//! Revoking a consent makes the next authorization from that client ask
//! again, and stops its refresh tokens (the refresh grant checks consent).
//! Access tokens already issued stay valid until they expire.

use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::sess::registry::authenticated_session;
use crate::web::request::RequestExt;
use axum::Json;
use axum::extract::{Path, Request};
use axum::http::StatusCode;
use serde::Serialize;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountConsentSchema {
    pub client_id: String,
    /// Client name; `None` when the client no longer exists.
    pub client_name: Option<String>,
    pub scopes: Vec<String>,
    pub audiences: Vec<String>,
    pub granted_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AccountConsentsResponse {
    pub consents: Vec<AccountConsentSchema>,
}

#[utoipa::path(
    get,
    path = "/account/consents",
    tags = ["Account"],
    summary = "List My OAuth Consents",
    description = "Applications the authenticated user has allowed to access their account, with the granted scopes.",
    responses(
        (status = 200, description = "OK", body = AccountConsentsResponse),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn get_consents(req: Request) -> Result<Json<AccountConsentsResponse>, ErrorResponse> {
    let session = authenticated_session(req.get_token()).await?;
    let consents = crate::data::db::list_active_oauth_consents(&session.user.id)
        .await
        .map_err(ErrorResponse::internal)?;

    let mut items = Vec::with_capacity(consents.len());
    for consent in consents {
        let client_name = crate::data::db::get_oauth_client_by_client_id(&consent.client_id)
            .await
            .map_err(ErrorResponse::internal)?
            .map(|client| client.name);
        items.push(AccountConsentSchema {
            client_id: consent.client_id,
            client_name,
            scopes: consent.scopes.0,
            audiences: consent.audiences.0,
            granted_at: consent.granted_at.to_rfc3339(),
        });
    }

    Ok(Json(AccountConsentsResponse { consents: items }))
}

#[utoipa::path(
    delete,
    path = "/account/consents/{client_id}",
    tags = ["Account"],
    summary = "Revoke My OAuth Consent",
    description = "Revoke the authenticated user's consent for an application. Its next authorization asks again and its refresh tokens stop working; issued access tokens stay valid until they expire.",
    params(("client_id" = String, Path, description = "OAuth client id")),
    responses(
        (status = 204, description = "Revoked"),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 404, description = "No active consent for this client", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn delete_consent(
    Path(client_id): Path<String>,
    req: Request,
) -> Result<StatusCode, ErrorResponse> {
    let session = authenticated_session(req.get_token()).await?;
    let audit_context = db::ent::TrustedAuditContext::application(
        db::ent::TrustedAuditActor::user(&session.user.id),
        crate::ctx::audit_request_from(req.extensions()),
    );

    crate::data::db::revoke_oauth_consent(&session.user.id, &client_id, audit_context)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::OAuthConsentNotFound))?;

    Ok(StatusCode::NO_CONTENT)
}
