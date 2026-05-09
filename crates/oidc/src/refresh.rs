use crate::OAuthError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use store::Store;

const FAMILY_KEY_PREFIX: &str = "oauth:refresh-family:";
const TOKEN_PREFIX: &str = "rt_";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RefreshTokenFamily {
    pub client_id: String,
    pub user_id: String,
    pub current_secret_hash: String,
    pub scope: String,
    pub audience: Option<String>,
    pub auth_time: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub generation: u64,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub fn family_key(family_id: &str) -> String {
    format!("{FAMILY_KEY_PREFIX}{family_id}")
}

pub fn refresh_token(family_id: &str, secret: &str) -> String {
    format!("{TOKEN_PREFIX}{family_id}.{secret}")
}

pub fn parse_refresh_token(token: &str) -> Option<(&str, &str)> {
    let token = token.strip_prefix(TOKEN_PREFIX)?;
    let (family_id, secret) = token.split_once('.')?;
    (!family_id.is_empty() && !secret.is_empty()).then_some((family_id, secret))
}

fn ttl_until(expires_at: DateTime<Utc>) -> Option<u64> {
    let now = Utc::now();
    (expires_at > now).then_some((expires_at - now).num_seconds().max(1) as u64)
}

fn new_secret() -> String {
    pw::generate_api_key()
}

pub async fn issue<S: Store>(
    store: &S,
    client_id: String,
    user_id: String,
    scope: String,
    audience: Option<String>,
    auth_time: DateTime<Utc>,
    ttl_secs: u64,
) -> Result<String, OAuthError> {
    let ttl_secs = ttl_secs.max(1);
    let family_id = ulid::Ulid::new().to_string();
    let secret = new_secret();
    let expires_at = Utc::now() + chrono::Duration::seconds(ttl_secs as i64);
    let family = RefreshTokenFamily {
        client_id,
        user_id,
        current_secret_hash: pw::hash_api_key(&secret),
        scope,
        audience,
        auth_time,
        expires_at,
        generation: 0,
        revoked_at: None,
    };

    store
        .set(&family_key(&family_id), &family, Some(ttl_secs))
        .await?;

    Ok(refresh_token(&family_id, &secret))
}

async fn revoke_family<S: Store>(
    store: &S,
    family_id: &str,
    current: &RefreshTokenFamily,
) -> Result<(), OAuthError> {
    if current.revoked_at.is_some() {
        return Ok(());
    }

    let mut revoked = current.clone();
    revoked.revoked_at = Some(Utc::now());
    let ttl = ttl_until(revoked.expires_at).unwrap_or(1);
    let _ = store
        .compare_and_swap(&family_key(family_id), current, &revoked, Some(ttl))
        .await?;
    Ok(())
}

pub async fn rotate<S: Store>(
    store: &S,
    token: &str,
    client_id: &str,
) -> Result<Option<(String, RefreshTokenFamily)>, OAuthError> {
    let Some((family_id, secret)) = parse_refresh_token(token.trim()) else {
        return Ok(None);
    };

    let key = family_key(family_id);
    let Some(current) = store.get::<RefreshTokenFamily>(&key).await? else {
        return Ok(None);
    };

    if current.revoked_at.is_some()
        || current.expires_at <= Utc::now()
        || current.client_id != client_id
    {
        return Ok(None);
    }

    let supplied_secret_hash = pw::hash_api_key(secret);
    if supplied_secret_hash != current.current_secret_hash {
        revoke_family(store, family_id, &current).await?;
        return Ok(None);
    }

    let next_secret = new_secret();
    let mut next = current.clone();
    next.current_secret_hash = pw::hash_api_key(&next_secret);
    next.generation = next.generation.saturating_add(1);
    let ttl = ttl_until(next.expires_at).unwrap_or(1);
    let swapped = store
        .compare_and_swap(&key, &current, &next, Some(ttl))
        .await?;

    if !swapped {
        if let Some(latest) = store.get::<RefreshTokenFamily>(&key).await?
            && latest.revoked_at.is_none()
            && latest.current_secret_hash != supplied_secret_hash
        {
            revoke_family(store, family_id, &latest).await?;
        }
        return Ok(None);
    }

    Ok(Some((refresh_token(family_id, &next_secret), next)))
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;
    use store::MemoryStore;

    #[tokio::test]
    async fn refresh_token_rotates_once_and_replay_revokes_family() {
        let store = MemoryStore::new();
        let token = issue(
            &store,
            "client-1".to_string(),
            "user-1".to_string(),
            "openid offline_access".to_string(),
            Some("gateway".to_string()),
            Utc::now(),
            60,
        )
        .await
        .unwrap();

        let rotated = rotate(&store, &token, "client-1").await.unwrap();
        assert!(rotated.is_some());

        let replay = rotate(&store, &token, "client-1").await.unwrap();
        assert!(replay.is_none());

        let (next_token, _) = rotated.unwrap();
        let after_replay = rotate(&store, &next_token, "client-1").await.unwrap();
        assert!(after_replay.is_none());
    }
}
