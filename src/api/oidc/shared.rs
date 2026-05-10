use crate::err::OAuthErrorResponse;
use axum::extract::Request;
use base64::Engine;
use http::header::AUTHORIZATION;

pub(super) use oidc::scopes::{
    AUTH_METHOD_CLIENT_SECRET_BASIC, AUTH_METHOD_CLIENT_SECRET_POST, AUTH_METHOD_NONE,
    AUTHORIZATION_CODE_TTL_SECS, GRANT_AUTHORIZATION_CODE, GRANT_CLIENT_CREDENTIALS,
    GRANT_REFRESH_TOKEN, OAUTH_CAN_INTROSPECT_ATTR, OAUTH_CAN_REVOKE_ATTR, OAUTH_INTROSPECT_SCOPE,
    OAUTH_REVOKE_SCOPE, OAUTH_TOKENS_GRANT, PKCE_METHOD_S256, RESPONSE_CODE, SCOPE_EMAIL,
    SCOPE_OFFLINE_ACCESS, SCOPE_OPENID, SCOPE_PROFILE, contains as scope_contains,
    parse_space_delimited, resolve_audience, resolve_scopes,
};

#[derive(Debug)]
pub(super) struct ClientCredentials {
    pub(super) client_id: String,
    pub(super) client_secret: String,
    pub(super) auth_method: &'static str,
}

pub(super) type OAuthResult<T> = Result<T, OAuthErrorResponse>;

pub(super) fn oauth_bad_request(message: impl Into<String>) -> OAuthErrorResponse {
    OAuthErrorResponse::bad_request(message)
}

pub(super) fn oauth_invalid_client(message: impl Into<String>) -> OAuthErrorResponse {
    OAuthErrorResponse::invalid_client(message)
}

pub(super) fn oauth_invalid_grant(message: impl Into<String>) -> OAuthErrorResponse {
    OAuthErrorResponse::invalid_grant(message)
}

pub(super) fn oauth_unsupported_grant_type(message: impl Into<String>) -> OAuthErrorResponse {
    OAuthErrorResponse::unsupported_grant_type(message)
}

pub(super) fn extract_basic_client_credentials(
    req: &Request,
) -> OAuthResult<Option<ClientCredentials>> {
    let Some(value) = req.headers().get(AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| oauth_invalid_client("invalid authorization header"))?;
    let Some((scheme, encoded)) = value.split_once(' ') else {
        return Err(oauth_invalid_client("invalid authorization header"));
    };
    if !scheme.eq_ignore_ascii_case("basic") {
        return Err(oauth_invalid_client(
            "unsupported client authentication scheme",
        ));
    }

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| oauth_invalid_client("invalid basic client authentication"))?;
    let decoded = String::from_utf8(decoded)
        .map_err(|_| oauth_invalid_client("invalid basic client authentication"))?;
    let Some((client_id, client_secret)) = decoded.split_once(':') else {
        return Err(oauth_invalid_client("invalid basic client authentication"));
    };

    let client_id = crate::etc::ext::percent_decode(client_id);
    let client_secret = crate::etc::ext::percent_decode(client_secret);
    if client_id.trim().is_empty() || client_secret.is_empty() {
        return Err(oauth_invalid_client("invalid basic client authentication"));
    }

    Ok(Some(ClientCredentials {
        client_id,
        client_secret,
        auth_method: AUTH_METHOD_CLIENT_SECRET_BASIC,
    }))
}

fn extract_post_client_credentials_parts(
    client_id: Option<&str>,
    client_secret: Option<&str>,
) -> OAuthResult<Option<ClientCredentials>> {
    match (client_id, client_secret) {
        (None, None) => Ok(None),
        (Some(client_id), Some(client_secret)) => {
            let client_id = client_id.trim().to_string();
            if client_id.is_empty() || client_secret.is_empty() {
                return Err(oauth_invalid_client("invalid client credentials"));
            }
            Ok(Some(ClientCredentials {
                client_id,
                client_secret: client_secret.to_string(),
                auth_method: AUTH_METHOD_CLIENT_SECRET_POST,
            }))
        }
        _ => Err(oauth_invalid_client(
            "client_id and client_secret must be provided together",
        )),
    }
}

pub(super) fn resolve_client_credentials_parts(
    basic: Option<ClientCredentials>,
    client_id: Option<&str>,
    client_secret: Option<&str>,
) -> OAuthResult<ClientCredentials> {
    let post = extract_post_client_credentials_parts(client_id, client_secret)?;
    match (basic, post) {
        (Some(_), Some(_)) => Err(oauth_bad_request(
            "multiple client authentication methods are not allowed",
        )),
        (Some(credentials), None) | (None, Some(credentials)) => Ok(credentials),
        (None, None) => Err(oauth_invalid_client("client authentication is required")),
    }
}

pub(super) async fn authenticate_oauth_client(
    credentials: &ClientCredentials,
) -> OAuthResult<db::ent::OAuthClient> {
    let Some(client) = crate::db::get_oauth_client_by_client_id(&credentials.client_id)
        .await
        .map_err(OAuthErrorResponse::internal)?
    else {
        return Err(oauth_invalid_client("invalid client credentials"));
    };

    if !client.enabled {
        return Err(oauth_invalid_client("client is disabled"));
    }

    if client.token_endpoint_auth_method == AUTH_METHOD_NONE {
        return Err(oauth_invalid_client(
            "public clients cannot authenticate with a client secret",
        ));
    }

    if client.token_endpoint_auth_method != credentials.auth_method {
        return Err(oauth_invalid_client(
            "client authentication method is not allowed",
        ));
    }

    let Some(stored_secret_hash) = client.client_secret_hash.as_deref() else {
        return Err(oauth_invalid_client("client secret is not configured"));
    };

    let supplied_secret_hash = pw::hash_api_key(&credentials.client_secret);
    if supplied_secret_hash != stored_secret_hash {
        return Err(oauth_invalid_client("invalid client credentials"));
    }

    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_scopes_accepts_registered_subset() {
        let allowed = vec!["read:users".to_string(), "write:users".to_string()];
        let scopes = resolve_scopes(Some("read:users read:users"), &allowed).unwrap();

        assert_eq!(scopes, vec!["read:users"]);
    }

    #[test]
    fn resolve_scopes_rejects_unregistered_scope() {
        let allowed = vec!["read:users".to_string()];

        assert!(resolve_scopes(Some("write:users"), &allowed).is_err());
    }

    #[test]
    fn resolve_audience_defaults_single_registered_audience() {
        let allowed = vec!["gateway".to_string()];
        let audience = resolve_audience(None, &allowed).unwrap();

        assert_eq!(audience.as_deref(), Some("gateway"));
    }

    #[test]
    fn resolve_audience_rejects_unregistered_audience() {
        let allowed = vec!["gateway".to_string()];

        assert!(resolve_audience(Some("admin-api"), &allowed).is_err());
    }
}
