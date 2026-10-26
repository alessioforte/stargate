//! OpenID Connect Back-Channel Logout 1.0: when a login session ends, every
//! relying party that received tokens for it (`SessionRecord::client_ids`)
//! and registered a `backchannel_logout_uri` gets a signed logout token.
//!
//! Delivery is in-process and best-effort: a few retries on network errors
//! and 5xx responses, nothing persisted. A session that simply expires is not
//! announced; relying parties rely on token expiry for those.

use super::http_client;
use crate::iam::sess::registry::SessionRecord;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::{JoinHandle, JoinSet};

/// Delays before each retry; one initial attempt plus these.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];
/// Concurrent deliveries across the process (mass revocations fan out).
const MAX_CONCURRENT_DELIVERIES: usize = 16;

static DELIVERY_PERMITS: Semaphore = Semaphore::const_new(MAX_CONCURRENT_DELIVERIES);

/// Logout notifications in flight for ended sessions.
pub struct LogoutDelivery(Option<JoinHandle<()>>);

impl LogoutDelivery {
    /// Wait up to `limit` for every relying party to be notified; delivery
    /// continues in the background afterwards.
    pub async fn wait(self, limit: Duration) {
        if let Some(handle) = self.0 {
            let _ = tokio::time::timeout(limit, handle).await;
        }
    }
}

/// Notify the relying parties linked to `sessions`, which have just ended.
pub fn notify(sessions: &[SessionRecord]) -> LogoutDelivery {
    let targets = sessions
        .iter()
        .flat_map(|session| {
            session.client_ids.iter().map(|client_id| Target {
                client_id: client_id.clone(),
                user_id: session.user_id.clone(),
                sid: session.id.clone(),
            })
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return LogoutDelivery(None);
    }

    LogoutDelivery(Some(tokio::spawn(async move {
        let mut deliveries = JoinSet::new();
        for target in targets {
            deliveries.spawn(async move {
                let Ok(_permit) = DELIVERY_PERMITS.acquire().await else {
                    return;
                };
                deliver(target).await;
            });
        }
        while deliveries.join_next().await.is_some() {}
    })))
}

struct Target {
    client_id: String,
    user_id: String,
    sid: String,
}

async fn deliver(target: Target) {
    let client = match crate::data::db::get_oauth_client_by_client_id(&target.client_id).await {
        Ok(Some(client)) => client,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(client_id = %target.client_id, "back-channel logout: client lookup failed: {error}");
            return;
        }
    };
    let Some(uri) = client.backchannel_logout_uri.as_deref() else {
        return;
    };

    // `sub` is the ID token's subject; `sid` is always sent, which also
    // satisfies `backchannel_logout_session_required`.
    let claims = oidc::logout::logout_token_claims(
        &jwt::issuer_from_env(),
        &target.user_id,
        &target.client_id,
        Some(&target.sid),
        chrono::Utc::now().timestamp(),
    );
    let token = match crate::iam::auth::jwt::jwt_config().generate_logout_token(&claims) {
        Ok(token) => token,
        Err(error) => {
            tracing::error!(client_id = %target.client_id, "back-channel logout: signing failed: {error}");
            return;
        }
    };

    for (attempt, delay) in std::iter::once(None)
        .chain(RETRY_DELAYS.iter().copied().map(Some))
        .enumerate()
    {
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        match post(uri, &token).await {
            Outcome::Delivered => {
                tracing::debug!(client_id = %target.client_id, sid = %target.sid, "back-channel logout delivered");
                return;
            }
            Outcome::Rejected(status) => {
                tracing::warn!(client_id = %target.client_id, %status, "back-channel logout rejected by the relying party");
                return;
            }
            Outcome::Retry(reason) => {
                tracing::warn!(client_id = %target.client_id, attempt = attempt + 1, "back-channel logout failed: {reason}");
            }
        }
    }
}

enum Outcome {
    Delivered,
    /// 4xx: the relying party refused the token; retrying cannot help.
    Rejected(reqwest::StatusCode),
    Retry(String),
}

async fn post(uri: &str, token: &str) -> Outcome {
    match http_client::client()
        .post(uri)
        .header(http::header::CACHE_CONTROL, "no-store")
        .form(&[("logout_token", token)])
        .send()
        .await
    {
        // RPs answer 200, or 204 from some frameworks (§2.8).
        Ok(response) if response.status().is_success() => Outcome::Delivered,
        Ok(response) if response.status().is_client_error() => Outcome::Rejected(response.status()),
        Ok(response) => Outcome::Retry(format!("HTTP {}", response.status())),
        Err(error) => Outcome::Retry(http_client::error_chain(&error)),
    }
}
