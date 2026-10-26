//! Client authentication at the token, introspection, and revocation
//! endpoints. A request presents at most one of: a client secret (HTTP Basic,
//! `client_secret_basic`, or form fields, `client_secret_post`), a signed
//! assertion (`private_key_jwt`, OIDC Core 9, RFC 7523), or only a `client_id`
//! (public clients, `none`).
//!
//! An assertion is verified with the keys the client registered (`jwks`, or the
//! document at `jwks_uri`); its `jti` is remembered until it expires, so each
//! assertion authenticates once.

use crate::err::oauth::{oauth_bad_request, oauth_invalid_client};
use crate::err::{OAuthErrorResponse, OAuthResult};
use crate::iam::oidc::client_jwks::{self, Freshness};
use axum::extract::Request;
use base64::Engine;
use http::header::AUTHORIZATION;
use oidc::client_assertion::{CLIENT_ASSERTION_TYPE_JWT_BEARER, ClientAssertionClaims};
use oidc::scopes::AUTH_METHOD_PRIVATE_KEY_JWT;
use oidc::scopes::{
    AUTH_METHOD_CLIENT_SECRET_BASIC, AUTH_METHOD_CLIENT_SECRET_POST, AUTH_METHOD_NONE,
};
use store::Store;

/// Seconds a replay marker outlives the assertion's `exp`, matching the
/// clock-skew leeway of the signature check.
const REPLAY_LEEWAY_SECS: i64 = 60;

/// Client authentication fields of a form-encoded request.
#[derive(Clone, Copy, Default)]
pub(super) struct ClientAuthForm<'a> {
    pub(super) client_id: Option<&'a str>,
    pub(super) client_secret: Option<&'a str>,
    pub(super) client_assertion_type: Option<&'a str>,
    pub(super) client_assertion: Option<&'a str>,
}

/// How a request identifies and authenticates its client.
pub(super) enum PresentedClient {
    /// Only a `client_id`: a public client.
    Public(String),
    Secret(ClientCredentials),
    Assertion {
        client_id: Option<String>,
        assertion: String,
    },
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// What the request presents, or `None` when it names no client at all.
pub(super) fn presented_client(
    basic: Option<ClientCredentials>,
    form: ClientAuthForm<'_>,
) -> OAuthResult<Option<PresentedClient>> {
    let assertion = match (
        nonempty(form.client_assertion_type),
        nonempty(form.client_assertion),
    ) {
        (None, None) => None,
        (Some(CLIENT_ASSERTION_TYPE_JWT_BEARER), Some(assertion)) => Some(assertion),
        (Some(_), Some(_)) => return Err(oauth_bad_request("unsupported client_assertion_type")),
        _ => {
            return Err(oauth_bad_request(
                "client_assertion and client_assertion_type must be sent together",
            ));
        }
    };
    let methods = usize::from(basic.is_some())
        + usize::from(form.client_secret.is_some())
        + usize::from(assertion.is_some());
    if methods > 1 {
        return Err(oauth_bad_request(
            "multiple client authentication methods are not allowed",
        ));
    }

    if let Some(credentials) = basic {
        if let Some(client_id) = nonempty(form.client_id)
            && client_id != credentials.client_id
        {
            return Err(oauth_invalid_client(
                "client_id does not match authentication",
            ));
        }
        return Ok(Some(PresentedClient::Secret(credentials)));
    }
    if form.client_secret.is_some() {
        let credentials =
            resolve_client_credentials_parts(None, form.client_id, form.client_secret)?;
        return Ok(Some(PresentedClient::Secret(credentials)));
    }
    if let Some(assertion) = assertion {
        return Ok(Some(PresentedClient::Assertion {
            client_id: nonempty(form.client_id).map(str::to_string),
            assertion: assertion.to_string(),
        }));
    }
    Ok(nonempty(form.client_id).map(|client_id| PresentedClient::Public(client_id.to_string())))
}

/// The authenticated client. `endpoint_path` (e.g. `/oauth/token`) is the
/// endpoint being called, an accepted assertion audience.
pub(super) async fn authenticate_client(
    presented: PresentedClient,
    endpoint_path: &str,
) -> OAuthResult<db::ent::OAuthClient> {
    match presented {
        PresentedClient::Public(client_id) => public_client(&client_id).await,
        PresentedClient::Secret(credentials) => authenticate_oauth_client(&credentials).await,
        PresentedClient::Assertion {
            client_id,
            assertion,
        } => assertion_client(client_id.as_deref(), &assertion, endpoint_path).await,
    }
}

async fn enabled_client(client_id: &str) -> OAuthResult<db::ent::OAuthClient> {
    let client = crate::data::db::get_oauth_client_by_client_id(client_id)
        .await
        .map_err(OAuthErrorResponse::internal)?
        .ok_or_else(|| oauth_invalid_client("invalid client"))?;
    if !client.enabled {
        return Err(oauth_invalid_client("client is disabled"));
    }
    Ok(client)
}

async fn public_client(client_id: &str) -> OAuthResult<db::ent::OAuthClient> {
    let client = enabled_client(client_id).await?;
    if client.token_endpoint_auth_method != AUTH_METHOD_NONE {
        return Err(oauth_invalid_client("client authentication is required"));
    }
    Ok(client)
}

async fn assertion_client(
    form_client_id: Option<&str>,
    assertion: &str,
    endpoint_path: &str,
) -> OAuthResult<db::ent::OAuthClient> {
    let client_id = oidc::client_assertion::claimed_client_id(assertion)
        .ok_or_else(|| oauth_invalid_client("client_assertion is not a JWT"))?;
    if form_client_id.is_some_and(|form_client_id| form_client_id != client_id) {
        return Err(oauth_invalid_client(
            "client_id does not match client_assertion",
        ));
    }
    let client = enabled_client(&client_id).await?;
    if client.token_endpoint_auth_method != AUTH_METHOD_PRIVATE_KEY_JWT {
        return Err(oauth_invalid_client(
            "client authentication method is not allowed",
        ));
    }

    let claims = verified_claims(&client, assertion).await?;
    let issuer = jwt::issuer_from_env();
    let base = crate::conf::public_base_url();
    let base = base.trim_end_matches('/');
    let audiences = [
        issuer,
        format!("{base}/oauth/token"),
        format!("{base}{endpoint_path}"),
    ];
    let now = chrono::Utc::now().timestamp();
    let accepted = oidc::client_assertion::accept(claims, &client.client_id, &audiences, now)
        .map_err(oauth_invalid_client)?;

    let replay_key = format!(
        "oauth:client-assertion:{}:{}",
        client.client_id,
        pw::hash_api_key(&accepted.jti)
    );
    let ttl = (accepted.exp - now + REPLAY_LEEWAY_SECS).max(1) as u64;
    if !crate::data::store::use_store()
        .set_if_absent(&replay_key, &true, Some(ttl))
        .await
        .map_err(OAuthErrorResponse::internal)?
    {
        return Err(oauth_invalid_client("client_assertion was already used"));
    }
    Ok(client)
}

/// The assertion's claims, signature-checked with the client's keys. Keys
/// from `jwks_uri` are refetched once when none of the cached ones verifies,
/// since the client may have rotated.
async fn verified_claims(
    client: &db::ent::OAuthClient,
    assertion: &str,
) -> OAuthResult<ClientAssertionClaims> {
    let invalid = |error: jwt::JwtError| {
        tracing::debug!(client_id = %client.client_id, %error, "client_assertion rejected");
        oauth_invalid_client(format!("invalid client_assertion: {error}"))
    };
    if let Some(jwks) = client.jwks.as_ref() {
        let jwks: jwt::JwkSet = serde_json::from_value(jwks.0.clone()).map_err(|error| {
            OAuthErrorResponse::internal(format!("stored client JWKS: {error}"))
        })?;
        return jwt::decode_with_jwk_set(assertion, &jwks).map_err(invalid);
    }
    let Some(uri) = client.jwks_uri.as_deref() else {
        return Err(oauth_invalid_client("client has no registered keys"));
    };
    let unavailable = |error: String| {
        tracing::warn!(client_id = %client.client_id, uri, "client jwks_uri: {error}");
        oauth_invalid_client("client keys are unavailable")
    };
    let cached = client_jwks::jwk_set(uri, Freshness::Cached)
        .await
        .map_err(unavailable)?;
    match jwt::decode_with_jwk_set(assertion, &cached) {
        Err(error) if *error.kind() == jwt::JwtErrorKind::InvalidSignature => {
            let fresh = client_jwks::jwk_set(uri, Freshness::Refetch)
                .await
                .map_err(unavailable)?;
            jwt::decode_with_jwk_set(assertion, &fresh).map_err(invalid)
        }
        result => result.map_err(invalid),
    }
}

/// A client secret presented with HTTP Basic or in the form.
#[derive(Debug)]
pub(super) struct ClientCredentials {
    pub(super) client_id: String,
    pub(super) client_secret: String,
    pub(super) auth_method: &'static str,
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

    let client_id = crate::web::request::percent_decode(client_id);
    let client_secret = crate::web::request::percent_decode(client_secret);
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
    let Some(client) = crate::data::db::get_oauth_client_by_client_id(&credentials.client_id)
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

    fn secret(client_id: &str) -> ClientCredentials {
        ClientCredentials {
            client_id: client_id.to_string(),
            client_secret: "s".to_string(),
            auth_method: oidc::scopes::AUTH_METHOD_CLIENT_SECRET_BASIC,
        }
    }

    fn kind(presented: OAuthResult<Option<PresentedClient>>) -> &'static str {
        match presented {
            Ok(None) => "none",
            Ok(Some(PresentedClient::Public(_))) => "public",
            Ok(Some(PresentedClient::Secret(_))) => "secret",
            Ok(Some(PresentedClient::Assertion { .. })) => "assertion",
            Err(_) => "error",
        }
    }

    #[test]
    fn one_authentication_method_at_a_time() {
        let assertion = ClientAuthForm {
            client_assertion_type: Some(CLIENT_ASSERTION_TYPE_JWT_BEARER),
            client_assertion: Some("h.p.s"),
            ..ClientAuthForm::default()
        };
        assert_eq!(
            kind(presented_client(None, ClientAuthForm::default())),
            "none"
        );
        assert_eq!(
            kind(presented_client(
                None,
                ClientAuthForm {
                    client_id: Some("c1"),
                    ..ClientAuthForm::default()
                }
            )),
            "public"
        );
        assert_eq!(
            kind(presented_client(
                Some(secret("c1")),
                ClientAuthForm::default()
            )),
            "secret"
        );
        assert_eq!(kind(presented_client(None, assertion)), "assertion");
        assert_eq!(
            kind(presented_client(Some(secret("c1")), assertion)),
            "error"
        );
        assert_eq!(
            kind(presented_client(
                None,
                ClientAuthForm {
                    client_secret: Some("s"),
                    ..assertion
                }
            )),
            "error"
        );
        assert_eq!(
            kind(presented_client(
                Some(secret("c1")),
                ClientAuthForm {
                    client_id: Some("c2"),
                    ..ClientAuthForm::default()
                }
            )),
            "error"
        );
    }

    #[test]
    fn assertion_type_is_checked() {
        for form in [
            ClientAuthForm {
                client_assertion_type: Some("urn:other"),
                client_assertion: Some("h.p.s"),
                ..ClientAuthForm::default()
            },
            ClientAuthForm {
                client_assertion: Some("h.p.s"),
                ..ClientAuthForm::default()
            },
            ClientAuthForm {
                client_assertion_type: Some(CLIENT_ASSERTION_TYPE_JWT_BEARER),
                ..ClientAuthForm::default()
            },
        ] {
            assert_eq!(kind(presented_client(None, form)), "error");
        }
    }
}
