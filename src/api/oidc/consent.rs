//! Consent step of `/oauth/authorize`.
//!
//! A request from a client the user has not consented to is parked with a
//! [`ConsentStage`](super::interactions::ConsentStage) bound to the signed-in
//! user, and the browser goes to the hosted auth app's consent page. The page
//! reads the request with `GET /oauth/interactions/{id}` and posts the
//! decision to `POST /oauth/interactions/{id}/consent`. Both require the same
//! user's session (cookie). Approval records the consent (audited); either
//! way the response is the URL that resumes the request, which then answers
//! the client (with `error=access_denied` after a denial) in the request's
//! response mode.
//!
//! The page needs JavaScript: unlike the login form, it has nothing to show
//! without the request details.

use super::interactions::{self, ConsentDecision, ConsentStage, PendingAuthorization};
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::sess::registry::authenticated_session;
use crate::web::request::RequestExt;
use axum::Json;
use axum::extract::{FromRequest, Path, Request};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsentClient {
    pub client_id: String,
    pub name: String,
    pub description: Option<String>,
    /// From the client's `attrs.logo_uri`, HTTP(S) only.
    pub logo_uri: Option<String>,
    pub client_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub tos_uri: Option<String>,
    /// Host the user will be sent back to, so they can recognize the app.
    pub redirect_host: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsentPromptResponse {
    pub client: ConsentClient,
    /// Scopes the client asks for (already limited to its registered ones).
    pub scopes: Vec<String>,
    /// Requested scopes the user granted to this client before.
    pub granted_scopes: Vec<String>,
    pub audience: Option<String>,
    /// The signed-in user, so they can tell which account they consent for.
    pub user_email: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ConsentDecisionRequest {
    pub approve: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsentDecisionResponse {
    /// Where the browser goes next: the request resumes and answers the
    /// client (`access_denied` after a denial).
    pub redirect_to: String,
}

#[utoipa::path(
    get,
    path = "/oauth/interactions/{interaction_id}",
    tags = ["OAuth"],
    summary = "Get Consent Request",
    description = "Details of an authorization request waiting for the signed-in user's consent. Used by the hosted consent page.",
    params(("interaction_id" = String, Path, description = "Parked authorization request id")),
    responses(
        (status = 200, description = "OK", body = ConsentPromptResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Expired, unknown, or another user's request", body = ErrorResponse),
    )
)]
pub async fn get_interaction(
    Path(interaction_id): Path<String>,
    req: Request,
) -> Result<Json<ConsentPromptResponse>, ErrorResponse> {
    let session = authenticated_session(req.get_token()).await?;
    let (pending, stage) = pending_consent(&interaction_id, &session.user.id).await?;
    let client = interaction_client(&pending).await?;

    let granted_scopes =
        crate::data::db::get_active_oauth_consent(&session.user.id, &client.client_id)
            .await
            .map_err(ErrorResponse::internal)?
            .map(|consent| {
                stage
                    .scopes
                    .iter()
                    .filter(|scope| consent.scopes.contains(scope))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

    Ok(Json(ConsentPromptResponse {
        client: consent_client(&client, pending.query.redirect_uri()),
        scopes: stage.scopes,
        granted_scopes,
        audience: stage.audience,
        user_email: session.user.email,
    }))
}

#[utoipa::path(
    post,
    path = "/oauth/interactions/{interaction_id}/consent",
    tags = ["OAuth"],
    summary = "Decide Consent Request",
    description = "Approve or deny an authorization request waiting for the signed-in user's consent. Approval records the consent; the response says where the browser goes next.",
    params(("interaction_id" = String, Path, description = "Parked authorization request id")),
    request_body = ConsentDecisionRequest,
    responses(
        (status = 200, description = "OK", body = ConsentDecisionResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Expired, unknown, or another user's request", body = ErrorResponse),
    )
)]
pub async fn post_consent(
    Path(interaction_id): Path<String>,
    req: Request,
) -> Result<Json<ConsentDecisionResponse>, ErrorResponse> {
    let session = authenticated_session(req.get_token()).await?;
    let audit_request = crate::ctx::audit_request_from(req.extensions());
    let Json(decision) = Json::<ConsentDecisionRequest>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;
    let user_id = session.user.id.as_str();
    let (pending, stage) = pending_consent(&interaction_id, user_id).await?;

    if !decision.approve {
        return decided(&interaction_id, user_id, ConsentDecision::Denied).await;
    }

    let client = interaction_client(&pending).await?;
    let previous = crate::data::db::get_active_oauth_consent(user_id, &client.client_id)
        .await
        .map_err(ErrorResponse::internal)?;
    let consent = db::ent::OAuthConsent::new(
        client.client_id.clone(),
        user_id.to_string(),
        merged(
            previous.as_ref().map(|consent| consent.scopes.as_slice()),
            &stage.scopes,
        ),
        merged(
            previous
                .as_ref()
                .map(|consent| consent.audiences.as_slice()),
            stage.audience.as_slice(),
        ),
        None,
        serde_json::json!({}),
    );
    let audit_context = db::ent::TrustedAuditContext::application(
        db::ent::TrustedAuditActor::user(user_id),
        audit_request,
    );
    crate::data::db::upsert_oauth_consent(consent, audit_context)
        .await
        .map_err(ErrorResponse::internal)?;

    decided(&interaction_id, user_id, ConsentDecision::Approved).await
}

/// Record the decision and answer with the URL that resumes the request.
async fn decided(
    interaction_id: &str,
    user_id: &str,
    decision: ConsentDecision,
) -> Result<Json<ConsentDecisionResponse>, ErrorResponse> {
    if !interactions::decide_consent(interaction_id, user_id, decision)
        .await
        .map_err(oauth_internal)?
    {
        return Err(ErrorResponse::new(ErrorCode::OAuthInteractionNotFound));
    }

    let redirect_to = format!(
        "{}{}",
        crate::conf::public_base_url().trim_end_matches('/'),
        interactions::resume_path(interaction_id)
    );
    Ok(Json(ConsentDecisionResponse { redirect_to }))
}

/// The parked request waiting for this user's consent. Other users' requests
/// look the same as missing ones.
async fn pending_consent(
    interaction_id: &str,
    user_id: &str,
) -> Result<(PendingAuthorization, ConsentStage), ErrorResponse> {
    let pending = interactions::get(interaction_id)
        .await
        .map_err(oauth_internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::OAuthInteractionNotFound))?;
    let stage = pending
        .consent
        .clone()
        .filter(|stage| stage.user_id == user_id)
        .ok_or_else(|| ErrorResponse::new(ErrorCode::OAuthInteractionNotFound))?;
    Ok((pending, stage))
}

async fn interaction_client(
    pending: &PendingAuthorization,
) -> Result<db::ent::OAuthClient, ErrorResponse> {
    let client_id = pending.query.client_id().unwrap_or_default();
    crate::data::db::get_oauth_client_by_client_id(client_id)
        .await
        .map_err(ErrorResponse::internal)?
        .filter(|client| client.enabled)
        .ok_or_else(|| ErrorResponse::new(ErrorCode::OAuthInteractionNotFound))
}

fn oauth_internal(error: crate::err::OAuthErrorResponse) -> ErrorResponse {
    ErrorResponse::internal(error.description())
}

/// Previously granted values plus the new ones, without duplicates.
fn merged(previous: Option<&[String]>, requested: &[String]) -> Vec<String> {
    let mut values: Vec<String> = previous.unwrap_or_default().to_vec();
    for value in requested {
        if !values.contains(value) {
            values.push(value.clone());
        }
    }
    values
}

fn consent_client(client: &db::ent::OAuthClient, redirect_uri: Option<&str>) -> ConsentClient {
    ConsentClient {
        client_id: client.client_id.clone(),
        name: client.name.clone(),
        description: client.description.clone(),
        logo_uri: http_attr(&client.attrs, "logo_uri"),
        client_uri: http_attr(&client.attrs, "client_uri"),
        policy_uri: http_attr(&client.attrs, "policy_uri"),
        tos_uri: http_attr(&client.attrs, "tos_uri"),
        redirect_host: redirect_uri
            .and_then(|uri| url::Url::parse(uri).ok())
            .and_then(|url| url.host_str().map(str::to_string)),
    }
}

/// A client-provided link, kept only when it is an absolute HTTP(S) URL so
/// the page never renders `javascript:` or other schemes.
fn http_attr(attrs: &serde_json::Value, key: &str) -> Option<String> {
    attrs
        .get(key)
        .and_then(serde_json::Value::as_str)
        .and_then(|value| url::Url::parse(value.trim()).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .map(|url| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_keeps_previous_values_and_adds_new_ones_once() {
        let previous = vec!["openid".to_string(), "email".to_string()];
        let requested = vec!["openid".to_string(), "profile".to_string()];

        assert_eq!(
            merged(Some(&previous), &requested),
            vec!["openid", "email", "profile"]
        );
        assert_eq!(merged(None, &requested), vec!["openid", "profile"]);
    }

    #[test]
    fn client_links_must_be_http_urls() {
        let attrs = serde_json::json!({
            "logo_uri": "https://app.example.com/logo.png",
            "client_uri": "javascript:alert(1)",
            "policy_uri": "not a url",
        });

        assert_eq!(
            http_attr(&attrs, "logo_uri").as_deref(),
            Some("https://app.example.com/logo.png")
        );
        assert!(http_attr(&attrs, "client_uri").is_none());
        assert!(http_attr(&attrs, "policy_uri").is_none());
    }

    #[test]
    fn redirect_host_is_shown_to_the_user() {
        let client = db::ent::OAuthClient::new(
            "client-1".to_string(),
            None,
            "Example".to_string(),
            None,
            "none".to_string(),
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            serde_json::json!({}),
        );

        let shown = consent_client(&client, Some("https://app.example.com/callback"));
        assert_eq!(shown.redirect_host.as_deref(), Some("app.example.com"));
    }
}
