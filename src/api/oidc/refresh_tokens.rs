use super::shared::OAuthResult;
use chrono::{DateTime, Utc};

pub(super) use oidc::refresh::RefreshTokenFamily;

pub(super) async fn issue(
    client_id: String,
    user_id: String,
    scope: String,
    audience: Option<String>,
    auth_time: DateTime<Utc>,
    nonce: Option<String>,
    ttl_secs: u64,
) -> OAuthResult<String> {
    oidc::refresh::issue(
        crate::etc::store::use_store(),
        client_id,
        user_id,
        scope,
        audience,
        auth_time,
        nonce,
        ttl_secs,
    )
    .await
    .map_err(Into::into)
}

pub(super) async fn rotate(
    token: &str,
    client_id: &str,
) -> OAuthResult<Option<(String, RefreshTokenFamily)>> {
    oidc::refresh::rotate(crate::etc::store::use_store(), token, client_id)
        .await
        .map_err(Into::into)
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refresh_token_rotates_once_and_replay_revokes_family() {
        let token = issue(
            "client-1".to_string(),
            "user-1".to_string(),
            "openid offline_access".to_string(),
            Some("gateway".to_string()),
            Utc::now(),
            Some("nonce-1".to_string()),
            60,
        )
        .await
        .unwrap();

        let rotated = rotate(&token, "client-1").await.unwrap();
        assert_eq!(
            rotated.as_ref().unwrap().1.nonce.as_deref(),
            Some("nonce-1")
        );

        let replay = rotate(&token, "client-1").await.unwrap();
        assert!(replay.is_none());

        let (next_token, _) = rotated.unwrap();
        let after_replay = rotate(&next_token, "client-1").await.unwrap();
        assert!(after_replay.is_none());
    }
}
