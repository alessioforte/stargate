pub mod backchannel_logout;
pub mod client_jwks;
pub mod codes;
pub mod http_client;
pub mod interactions;
pub mod refresh;
pub mod request;

/// Whether an enabled OAuth client is trusted for first-party account access.
pub(crate) async fn client_is_trusted_first_party(client_id: &str) -> bool {
    let client = match crate::data::db::get_oauth_client_by_client_id(client_id).await {
        Ok(Some(client)) => client,
        Ok(None) => return false,
        Err(err) => {
            tracing::error!(
                "Failed to resolve OAuth client for first-party access: {}",
                err
            );
            return false;
        }
    };

    client.enabled && oidc::consent::client_is_first_party(&client.attrs)
}
