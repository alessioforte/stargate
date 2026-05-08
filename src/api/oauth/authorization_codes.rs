use crate::err::ErrorResponse;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use store::Store;

const CODE_KEY_PREFIX: &str = "oauth:authorization-code:";
const CONSUMED_MARKER_TTL_SECS: u64 = 60;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(super) struct AuthorizationCodeRecord {
    pub(super) client_id: String,
    pub(super) user_id: String,
    pub(super) redirect_uri: String,
    pub(super) scope: String,
    pub(super) audience: Option<String>,
    pub(super) nonce: Option<String>,
    pub(super) code_challenge: String,
    pub(super) code_challenge_method: String,
    pub(super) auth_time: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
enum StoredAuthorizationCode {
    Pending(Box<AuthorizationCodeRecord>),
    Consumed,
}

fn code_key(code_hash: &str) -> String {
    format!("{CODE_KEY_PREFIX}{code_hash}")
}

pub(super) async fn store(
    code_hash: &str,
    record: AuthorizationCodeRecord,
    ttl_secs: u64,
) -> Result<(), ErrorResponse> {
    crate::etc::store::use_store()
        .set(
            &code_key(code_hash),
            &StoredAuthorizationCode::Pending(Box::new(record)),
            Some(ttl_secs),
        )
        .await
        .map_err(ErrorResponse::internal)
}

pub(super) async fn get(code_hash: &str) -> Result<Option<AuthorizationCodeRecord>, ErrorResponse> {
    let key = code_key(code_hash);
    let store = crate::etc::store::use_store();
    let Some(current) = store
        .get::<StoredAuthorizationCode>(&key)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Ok(None);
    };

    let StoredAuthorizationCode::Pending(record) = current else {
        return Ok(None);
    };

    Ok(Some(*record))
}

pub(super) async fn consume(
    code_hash: &str,
    record: &AuthorizationCodeRecord,
) -> Result<bool, ErrorResponse> {
    let key = code_key(code_hash);
    let store = crate::etc::store::use_store();
    let expected = StoredAuthorizationCode::Pending(Box::new(record.clone()));
    let consumed = StoredAuthorizationCode::Consumed;
    let swapped = store
        .compare_and_swap(&key, &expected, &consumed, Some(CONSUMED_MARKER_TTL_SECS))
        .await
        .map_err(ErrorResponse::internal)?;
    if !swapped {
        return Ok(false);
    }

    if let Err(error) = store.delete(&key).await {
        tracing::warn!(key, %error, "Failed to delete consumed OAuth authorization code");
    }

    Ok(true)
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;

    fn record() -> AuthorizationCodeRecord {
        AuthorizationCodeRecord {
            client_id: "client-1".to_string(),
            user_id: "user-1".to_string(),
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
        let code_hash = format!("test-{}", ulid::Ulid::new());

        store(&code_hash, record(), 60).await.unwrap();

        let first = get(&code_hash).await.unwrap();
        let first_consumed = consume(&code_hash, first.as_ref().unwrap()).await.unwrap();
        let second = get(&code_hash).await.unwrap();

        assert!(first.is_some());
        assert!(first_consumed);
        assert!(second.is_none());
    }
}
