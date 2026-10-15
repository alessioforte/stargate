use super::shared::OAuthResult;

pub(super) use oidc::codes::{AuthorizationCodeRecord, CodeLookup, RedeemedGrant};

pub(super) async fn store(
    code_hash: &str,
    record: AuthorizationCodeRecord,
    ttl_secs: u64,
) -> OAuthResult<()> {
    oidc::codes::store(crate::etc::store::use_store(), code_hash, record, ttl_secs)
        .await
        .map_err(Into::into)
}

pub(super) async fn get(code_hash: &str) -> OAuthResult<CodeLookup> {
    oidc::codes::get(crate::etc::store::use_store(), code_hash)
        .await
        .map_err(Into::into)
}

pub(super) async fn consume(
    code_hash: &str,
    record: &AuthorizationCodeRecord,
) -> OAuthResult<bool> {
    oidc::codes::consume(crate::etc::store::use_store(), code_hash, record)
        .await
        .map_err(Into::into)
}

pub(super) async fn record_redemption(code_hash: &str, grant: RedeemedGrant) -> OAuthResult<()> {
    oidc::codes::record_redemption(crate::etc::store::use_store(), code_hash, grant)
        .await
        .map_err(Into::into)
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;

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
            auth_time: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn consume_is_single_use() {
        let code_hash = format!("test-{}", ulid::Ulid::generate());

        store(&code_hash, record(), 60).await.unwrap();

        let CodeLookup::Pending(first) = get(&code_hash).await.unwrap() else {
            panic!("expected a pending code");
        };
        let first_consumed = consume(&code_hash, &first).await.unwrap();
        let second = get(&code_hash).await.unwrap();

        assert!(first_consumed);
        assert_eq!(second, CodeLookup::Redeemed(None));
    }
}
