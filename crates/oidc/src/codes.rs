use crate::OAuthError;
use crate::authentication::Authentication;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use store::Store;

const CODE_KEY_PREFIX: &str = "oauth:authorization-code:";
/// How long a redeemed code is remembered, so a replay can still be detected
/// and the tokens it issued revoked (RFC 6749 §4.1.2). Matches the code TTL.
const REDEEMED_MARKER_TTL_SECS: u64 = crate::scopes::AUTHORIZATION_CODE_TTL_SECS as u64;

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
    pub sid: Option<String>,
    pub authentication: Authentication,
}

/// Tokens issued for a redeemed code, revoked if the code is replayed.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct RedeemedGrant {
    pub access_jti: Option<String>,
    pub access_exp: Option<usize>,
    pub refresh_family_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
enum StoredAuthorizationCode {
    Pending(Box<AuthorizationCodeRecord>),
    Consumed,
    Redeemed(RedeemedGrant),
}

/// What a presented authorization code refers to.
#[derive(Clone, Debug, PartialEq)]
pub enum CodeLookup {
    Missing,
    Pending(Box<AuthorizationCodeRecord>),
    /// Already exchanged. Carries the issued tokens once they are recorded.
    Redeemed(Option<RedeemedGrant>),
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

pub async fn get<S: Store>(store: &S, code_hash: &str) -> Result<CodeLookup, OAuthError> {
    let key = code_key(code_hash);
    Ok(match store.get::<StoredAuthorizationCode>(&key).await? {
        None => CodeLookup::Missing,
        Some(StoredAuthorizationCode::Pending(record)) => CodeLookup::Pending(record),
        Some(StoredAuthorizationCode::Consumed) => CodeLookup::Redeemed(None),
        Some(StoredAuthorizationCode::Redeemed(grant)) => CodeLookup::Redeemed(Some(grant)),
    })
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
        .compare_and_swap(&key, &expected, &consumed, Some(REDEEMED_MARKER_TTL_SECS))
        .await?;
    Ok(swapped)
}

/// Remember the tokens issued for a consumed code so a later replay can
/// revoke them.
pub async fn record_redemption<S: Store>(
    store: &S,
    code_hash: &str,
    grant: RedeemedGrant,
) -> Result<(), OAuthError> {
    store
        .set(
            &code_key(code_hash),
            &StoredAuthorizationCode::Redeemed(grant),
            Some(REDEEMED_MARKER_TTL_SECS),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::AuthorizationCodeRecord;

    pub(crate) fn record() -> AuthorizationCodeRecord {
        AuthorizationCodeRecord {
            client_id: "client-1".to_string(),
            user_id: "user-1".to_string(),
            sid: Some("session-1".to_string()),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: "openid email".to_string(),
            audience: Some("gateway".to_string()),
            nonce: Some("nonce-1".to_string()),
            code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string(),
            code_challenge_method: "S256".to_string(),
            auth_time: chrono::Utc::now(),
            authentication: crate::authentication::Authentication {
                amr: vec!["pwd".to_string()],
                acr: crate::authentication::ACR_SINGLE_FACTOR.to_string(),
            },
        }
    }
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
            authentication: crate::authentication::Authentication {
                amr: vec!["pwd".to_string()],
                acr: crate::authentication::ACR_SINGLE_FACTOR.to_string(),
            },
        }
    }

    #[tokio::test]
    async fn consume_is_single_use() {
        let store = MemoryStore::new();
        let code_hash = format!("test-{}", ulid::Ulid::generate());

        super::store(&store, &code_hash, record(), 60)
            .await
            .unwrap();

        let CodeLookup::Pending(first) = get(&store, &code_hash).await.unwrap() else {
            panic!("expected a pending code");
        };
        let first_consumed = consume(&store, &code_hash, &first).await.unwrap();
        let second_consumed = consume(&store, &code_hash, &first).await.unwrap();
        let second = get(&store, &code_hash).await.unwrap();

        assert!(first_consumed);
        assert!(!second_consumed);
        assert_eq!(second, CodeLookup::Redeemed(None));
    }

    #[tokio::test]
    async fn redeemed_code_keeps_issued_grant() {
        let store = MemoryStore::new();
        let code_hash = format!("test-{}", ulid::Ulid::generate());
        let grant = RedeemedGrant {
            access_jti: Some("jti-1".to_string()),
            access_exp: Some(1),
            refresh_family_id: None,
        };

        super::store(&store, &code_hash, record(), 60)
            .await
            .unwrap();
        record_redemption(&store, &code_hash, grant.clone())
            .await
            .unwrap();

        assert_eq!(
            get(&store, &code_hash).await.unwrap(),
            CodeLookup::Redeemed(Some(grant))
        );
    }
}
