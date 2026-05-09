use crate::err::ErrorResponse;
use chrono::{DateTime, Utc};

pub(super) use oidc::refresh::RefreshTokenFamily;

pub(super) async fn issue(
    client_id: String,
    user_id: String,
    scope: String,
    audience: Option<String>,
    auth_time: DateTime<Utc>,
    ttl_secs: u64,
) -> Result<String, ErrorResponse> {
    oidc::refresh::issue(
        crate::etc::store::use_store(),
        client_id,
        user_id,
        scope,
        audience,
        auth_time,
        ttl_secs,
    )
    .await
    .map_err(ErrorResponse::from)
}

pub(super) async fn rotate(
    token: &str,
    client_id: &str,
) -> Result<Option<(String, RefreshTokenFamily)>, ErrorResponse> {
    oidc::refresh::rotate(crate::etc::store::use_store(), token, client_id)
        .await
        .map_err(ErrorResponse::from)
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
            60,
        )
        .await
        .unwrap();

        let rotated = rotate(&token, "client-1").await.unwrap();
        assert!(rotated.is_some());

        let replay = rotate(&token, "client-1").await.unwrap();
        assert!(replay.is_none());

        let (next_token, _) = rotated.unwrap();
        let after_replay = rotate(&next_token, "client-1").await.unwrap();
        assert!(after_replay.is_none());
    }
}
