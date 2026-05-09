use crate::err::ErrorResponse;

pub(super) use oidc::codes::AuthorizationCodeRecord;

pub(super) async fn store(
    code_hash: &str,
    record: AuthorizationCodeRecord,
    ttl_secs: u64,
) -> Result<(), ErrorResponse> {
    oidc::codes::store(crate::etc::store::use_store(), code_hash, record, ttl_secs)
        .await
        .map_err(ErrorResponse::from)
}

pub(super) async fn get(code_hash: &str) -> Result<Option<AuthorizationCodeRecord>, ErrorResponse> {
    oidc::codes::get(crate::etc::store::use_store(), code_hash)
        .await
        .map_err(ErrorResponse::from)
}

pub(super) async fn consume(
    code_hash: &str,
    record: &AuthorizationCodeRecord,
) -> Result<bool, ErrorResponse> {
    oidc::codes::consume(crate::etc::store::use_store(), code_hash, record)
        .await
        .map_err(ErrorResponse::from)
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
            auth_time: chrono::Utc::now(),
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
