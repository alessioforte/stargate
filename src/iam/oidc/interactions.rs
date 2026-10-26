//! Authorization requests parked while the user signs in, consents, or adds
//! a factor.
//!
//! When `/oauth/authorize` needs an interactive login, the request is stored
//! under an unguessable id that resumes it after sign-in. When the signed-in
//! user has not consented to the client, it is parked again with a
//! [`ConsentStage`] bound to that user; when the client asks for multi-factor
//! `acr_values` and the session has a single factor, with a [`StepUpStage`].
//! The browser side (cookie, hosted pages, resume path) is in
//! `api::oidc::interactions`.

use super::request::AuthorizationRequest;
use crate::err::{OAuthErrorResponse, OAuthResult};
use serde::{Deserialize, Serialize};
use store::Store;

const KEY_PREFIX: &str = "oauth:authorize-interaction:";
pub const INTERACTION_TTL_SECS: u64 = 600;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingAuthorization {
    pub query: AuthorizationRequest,
    /// Unix seconds when the request was parked. A session whose `auth_time`
    /// is not older than this was authenticated for this request.
    pub created_at: i64,
    /// Present when the request waits for the user's consent.
    pub consent: Option<ConsentStage>,
    /// Present when the request waits for the user to add a factor.
    pub step_up: Option<StepUpStage>,
}

/// A request parked for an MFA step-up. The login checks already passed for
/// `user_id`, so resuming only needs that same user's session.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StepUpStage {
    pub user_id: String,
}

/// A request parked for consent. The login checks already passed for
/// `user_id`, so resuming only needs that same user's session.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConsentStage {
    pub user_id: String,
    /// Scopes and audience the client asked for, after validation.
    pub scopes: Vec<String>,
    pub audience: Option<String>,
    /// Set by the user's choice on the consent page.
    pub decision: Option<ConsentDecision>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConsentDecision {
    /// Resuming issues a code, even with `prompt=consent`.
    Approved,
    /// Resuming answers the client with `access_denied`.
    Denied,
}

fn key(id: &str) -> String {
    format!("{KEY_PREFIX}{id}")
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub async fn park(query: AuthorizationRequest) -> OAuthResult<String> {
    park_with(query, None, None).await
}

pub async fn park_for_consent(
    query: AuthorizationRequest,
    stage: ConsentStage,
) -> OAuthResult<String> {
    park_with(query, Some(stage), None).await
}

pub async fn park_for_step_up(
    query: AuthorizationRequest,
    stage: StepUpStage,
) -> OAuthResult<String> {
    park_with(query, None, Some(stage)).await
}

async fn park_with(
    query: AuthorizationRequest,
    consent: Option<ConsentStage>,
    step_up: Option<StepUpStage>,
) -> OAuthResult<String> {
    let id = format!("{}_{}", ulid::Ulid::generate(), pw::generate_api_key());
    let pending = PendingAuthorization {
        query,
        created_at: chrono::Utc::now().timestamp(),
        consent,
        step_up,
    };
    save(&id, &pending).await?;
    Ok(id)
}

async fn save(id: &str, pending: &PendingAuthorization) -> OAuthResult<()> {
    crate::data::store::use_store()
        .set(&key(id), pending, Some(INTERACTION_TTL_SECS))
        .await
        .map_err(OAuthErrorResponse::internal)
}

/// Record the user's decision on a request parked for consent; resuming it
/// answers the client accordingly. Returns false when there is no such
/// request for `user_id`.
pub async fn decide_consent(
    id: &str,
    user_id: &str,
    decision: ConsentDecision,
) -> OAuthResult<bool> {
    let Some(mut pending) = get(id).await? else {
        return Ok(false);
    };
    let Some(stage) = pending
        .consent
        .as_mut()
        .filter(|stage| stage.user_id == user_id)
    else {
        return Ok(false);
    };
    stage.decision = Some(decision);
    save(id, &pending).await?;
    Ok(true)
}

pub async fn get(id: &str) -> OAuthResult<Option<PendingAuthorization>> {
    if !valid_id(id) {
        return Ok(None);
    }
    crate::data::store::use_store()
        .get(&key(id))
        .await
        .map_err(OAuthErrorResponse::internal)
}

/// Load and delete a parked request so it resumes at most once.
pub async fn take(id: &str) -> OAuthResult<Option<PendingAuthorization>> {
    let Some(pending) = get(id).await? else {
        return Ok(None);
    };
    let deleted = crate::data::store::use_store()
        .delete(&key(id))
        .await
        .map_err(OAuthErrorResponse::internal)?;
    if !deleted {
        return Ok(None);
    }
    Ok(Some(pending))
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn consent_approval_is_bound_to_the_parked_user() {
        let stage = ConsentStage {
            user_id: "user-1".to_string(),
            scopes: vec!["openid".to_string()],
            audience: None,
            decision: None,
        };
        let id = park_for_consent(AuthorizationRequest::default(), stage)
            .await
            .unwrap();
        let decision = || async { get(&id).await.unwrap().unwrap().consent.unwrap().decision };

        assert!(
            !decide_consent(&id, "user-2", ConsentDecision::Approved)
                .await
                .unwrap()
        );
        assert_eq!(decision().await, None);

        assert!(
            decide_consent(&id, "user-1", ConsentDecision::Denied)
                .await
                .unwrap()
        );
        assert_eq!(decision().await, Some(ConsentDecision::Denied));
        assert!(
            !decide_consent("missing", "user-1", ConsentDecision::Approved)
                .await
                .unwrap()
        );
    }
}
