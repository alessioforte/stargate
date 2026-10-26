use crate::err::OAuthErrorResponse;
use chrono::{DateTime, Utc};

pub use oidc::refresh::RefreshTokenFamily;

#[allow(clippy::too_many_arguments)] // complete refresh-family context
pub async fn issue(
    client_id: String,
    user_id: String,
    sid: Option<String>,
    scope: String,
    audience: Option<String>,
    auth_time: DateTime<Utc>,
    nonce: Option<String>,
    authentication: oidc::authentication::Authentication,
    ttl_secs: u64,
) -> Result<String, OAuthErrorResponse> {
    oidc::refresh::issue(
        crate::data::store::use_store(),
        client_id,
        user_id,
        sid,
        scope,
        audience,
        auth_time,
        nonce,
        authentication,
        ttl_secs,
    )
    .await
    .map_err(Into::into)
}

pub async fn rotate(
    token: &str,
    client_id: &str,
) -> Result<Option<(String, RefreshTokenFamily)>, OAuthErrorResponse> {
    oidc::refresh::rotate(crate::data::store::use_store(), token, client_id)
        .await
        .map_err(Into::into)
}

/// Drop a refresh-token family so none of its tokens rotate again.
pub async fn revoke_family(family_id: &str) -> Result<(), OAuthErrorResponse> {
    use store::Store;

    crate::data::store::use_store()
        .delete(&oidc::refresh::family_key(family_id))
        .await
        .map_err(OAuthErrorResponse::internal)?;
    Ok(())
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refresh_token_rotates_once_and_replay_revokes_family() {
        let token = issue(
            "client-1".to_string(),
            "user-1".to_string(),
            Some("session-1".to_string()),
            "openid offline_access".to_string(),
            Some("gateway".to_string()),
            Utc::now(),
            Some("nonce-1".to_string()),
            oidc::authentication::Authentication {
                amr: vec!["pwd".to_string()],
                acr: oidc::authentication::ACR_SINGLE_FACTOR.to_string(),
            },
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
