use crate::OAuthError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use store::Store;

const CODE_KEY_PREFIX: &str = "oauth:authorization-code:";
const CONSUMED_MARKER_TTL_SECS: u64 = 60;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AuthorizationCodeRecord {
    pub client_id: String,
    pub user_id: String,
    pub redirect_uri: String,
    pub scope: String,
    pub audience: Option<String>,
    pub nonce: Option<String>,
    pub code_challenge: String,
    pub code_challenge_method: String,
    pub auth_time: DateTime<Utc>,
    #[serde(default)]
    pub sid: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
enum StoredAuthorizationCode {
    Pending(Box<AuthorizationCodeRecord>),
    Consumed,
}

pub fn code_key(code_hash: &str) -> String {
    format!("{CODE_KEY_PREFIX}{code_hash}")
}

pub fn authorization_code() -> String {
    format!("code_{}_{}", ulid::Ulid::generate(), pw::generate_api_key())
}

pub async fn store<S: Store>(
    store: &S,
    code_hash: &str,
    record: AuthorizationCodeRecord,
    ttl_secs: u64,
) -> Result<(), OAuthError> {
    store
        .set(
            &code_key(code_hash),
            &StoredAuthorizationCode::Pending(Box::new(record)),
            Some(ttl_secs),
        )
        .await?;
    Ok(())
}

pub async fn get<S: Store>(
    store: &S,
    code_hash: &str,
) -> Result<Option<AuthorizationCodeRecord>, OAuthError> {
    let key = code_key(code_hash);
    let Some(current) = store.get::<StoredAuthorizationCode>(&key).await? else {
        return Ok(None);
    };

    let StoredAuthorizationCode::Pending(record) = current else {
        return Ok(None);
    };

    Ok(Some(*record))
}

pub async fn consume<S: Store>(
    store: &S,
    code_hash: &str,
    record: &AuthorizationCodeRecord,
) -> Result<bool, OAuthError> {
    let key = code_key(code_hash);
    let expected = StoredAuthorizationCode::Pending(Box::new(record.clone()));
    let consumed = StoredAuthorizationCode::Consumed;
    let swapped = store
        .compare_and_swap(&key, &expected, &consumed, Some(CONSUMED_MARKER_TTL_SECS))
        .await?;
    if !swapped {
        return Ok(false);
    }

    if let Err(error) = store.delete(&key).await {
        tracing::warn!(key, %error, "failed to delete consumed OAuth authorization code");
    }

    Ok(true)
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;
    use store::MemoryStore;

    fn record() -> AuthorizationCodeRecord {
        AuthorizationCodeRecord {
            client_id: "client-1".to_string(),
            user_id: "user-1".to_string(),
            sid: Some("session-1".to_string()),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: "openid email".to_string(),
            audience: Some("gateway".to_string()),
            nonce: Some("nonce-1".to_string()),
            code_challenge: "challenge".to_string(),
            code_challenge_method: "S256".to_string(),
            auth_time: Utc::now(),
        }
    }

    #[tokio::test]
    async fn consume_is_single_use() {
        let store = MemoryStore::new();
        let code_hash = format!("test-{}", ulid::Ulid::generate());

        super::store(&store, &code_hash, record(), 60)
            .await
            .unwrap();

        let first = get(&store, &code_hash).await.unwrap();
        let first_consumed = consume(&store, &code_hash, first.as_ref().unwrap())
            .await
            .unwrap();
        let second = get(&store, &code_hash).await.unwrap();

        assert!(first.is_some());
        assert!(first_consumed);
        assert!(second.is_none());
    }
}
