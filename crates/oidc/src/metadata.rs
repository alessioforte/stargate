use crate::scopes::{
    AUTH_METHOD_CLIENT_SECRET_BASIC, AUTH_METHOD_CLIENT_SECRET_POST, AUTH_METHOD_NONE,
    GRANT_AUTHORIZATION_CODE, GRANT_CLIENT_CREDENTIALS, GRANT_REFRESH_TOKEN, PKCE_METHOD_S256,
    RESPONSE_CODE, SCOPE_EMAIL, SCOPE_OFFLINE_ACCESS, SCOPE_OPENID, SCOPE_PROFILE,
};
use jwt::{AlgorithmParameters, EllipticCurve, PublicKeyUse};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct PublicJwk {
    kty: String,
    #[serde(rename = "use", skip_serializing_if = "Option::is_none")]
    key_use: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    n: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    e: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    x: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    y: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Jwks {
    keys: Vec<PublicJwk>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OAuthAuthorizationServerMetadata {
    issuer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    authorization_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_endpoint: Option<String>,
    introspection_endpoint: String,
    revocation_endpoint: String,
    jwks_uri: String,
    response_types_supported: Vec<String>,
    grant_types_supported: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_endpoint_auth_methods_supported: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code_challenge_methods_supported: Option<Vec<String>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OpenIdConfiguration {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    userinfo_endpoint: Option<String>,
    jwks_uri: String,
    response_types_supported: Vec<String>,
    grant_types_supported: Vec<String>,
    subject_types_supported: Vec<String>,
    id_token_signing_alg_values_supported: Vec<String>,
    scopes_supported: Vec<String>,
    claims_supported: Vec<String>,
    code_challenge_methods_supported: Vec<String>,
    token_endpoint_auth_methods_supported: Vec<String>,
}

pub fn endpoint_base_url(issuer: &str, configured_base_url: Option<&str>, port: &str) -> String {
    configured_base_url
        .filter(|base_url| !base_url.trim().is_empty())
        .map(str::to_string)
        .or_else(|| issuer.contains("://").then(|| issuer.to_string()))
        .unwrap_or_else(|| format!("http://localhost:{port}"))
}

pub fn endpoint_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn public_key_use_name(jwk: &jwt::Jwk) -> Option<&str> {
    jwk.common.public_key_use.as_ref().map(|value| match value {
        PublicKeyUse::Signature => "sig",
        PublicKeyUse::Encryption => "enc",
        PublicKeyUse::Other(other) => other.as_str(),
    })
}

fn key_algorithm_name(jwk: &jwt::Jwk) -> Option<String> {
    jwk.common
        .key_algorithm
        .map(|algorithm| algorithm.to_string())
}

fn elliptic_curve_name(curve: &EllipticCurve) -> Option<&'static str> {
    match curve {
        EllipticCurve::P256 => Some("P-256"),
        EllipticCurve::P384 => Some("P-384"),
        EllipticCurve::P521 => Some("P-521"),
        EllipticCurve::Ed25519 => Some("Ed25519"),
        _ => None,
    }
}

pub fn jwt_algorithm_name(algorithm: jwt::Algorithm) -> Option<&'static str> {
    match algorithm {
        jwt::Algorithm::HS256 => Some("HS256"),
        jwt::Algorithm::HS384 => Some("HS384"),
        jwt::Algorithm::HS512 => Some("HS512"),
        jwt::Algorithm::ES256 => Some("ES256"),
        jwt::Algorithm::ES384 => Some("ES384"),
        jwt::Algorithm::RS256 => Some("RS256"),
        jwt::Algorithm::RS384 => Some("RS384"),
        jwt::Algorithm::RS512 => Some("RS512"),
        jwt::Algorithm::PS256 => Some("PS256"),
        jwt::Algorithm::PS384 => Some("PS384"),
        jwt::Algorithm::PS512 => Some("PS512"),
        jwt::Algorithm::EdDSA => Some("EdDSA"),
        _ => None,
    }
}

fn public_jwk_from_jwk(jwk: jwt::Jwk) -> Option<PublicJwk> {
    let kid = jwk.common.key_id.clone();
    let key_use = public_key_use_name(&jwk).map(ToOwned::to_owned);
    let alg = key_algorithm_name(&jwk);

    match jwk.algorithm {
        AlgorithmParameters::RSA(params) => Some(PublicJwk {
            kty: "RSA".to_string(),
            key_use,
            kid,
            alg,
            n: Some(params.n),
            e: Some(params.e),
            crv: None,
            x: None,
            y: None,
        }),
        AlgorithmParameters::EllipticCurve(params) => Some(PublicJwk {
            kty: "EC".to_string(),
            key_use,
            kid,
            alg,
            n: None,
            e: None,
            crv: Some(elliptic_curve_name(&params.curve)?.to_string()),
            x: Some(params.x),
            y: Some(params.y),
        }),
        AlgorithmParameters::OctetKey(_) | AlgorithmParameters::OctetKeyPair(_) => None,
        _ => None,
    }
}

pub fn jwks_response(jwk_set: jwt::JwkSet) -> Jwks {
    let mut keys = Vec::with_capacity(jwk_set.keys.len());
    for jwk in jwk_set.keys {
        if let Some(key) = public_jwk_from_jwk(jwk) {
            keys.push(key);
        }
    }
    Jwks { keys }
}

pub fn authorization_server_metadata(
    issuer: String,
    base_url: &str,
) -> OAuthAuthorizationServerMetadata {
    OAuthAuthorizationServerMetadata {
        issuer,
        authorization_endpoint: Some(endpoint_url(base_url, "/oauth/authorize")),
        token_endpoint: Some(endpoint_url(base_url, "/oauth/token")),
        introspection_endpoint: endpoint_url(base_url, "/oauth/introspect"),
        revocation_endpoint: endpoint_url(base_url, "/oauth/revoke"),
        jwks_uri: endpoint_url(base_url, "/.well-known/jwks.json"),
        response_types_supported: vec![RESPONSE_CODE.to_string()],
        grant_types_supported: vec![
            GRANT_AUTHORIZATION_CODE.to_string(),
            GRANT_CLIENT_CREDENTIALS.to_string(),
            GRANT_REFRESH_TOKEN.to_string(),
        ],
        token_endpoint_auth_methods_supported: Some(vec![
            AUTH_METHOD_CLIENT_SECRET_BASIC.to_string(),
            AUTH_METHOD_CLIENT_SECRET_POST.to_string(),
            AUTH_METHOD_NONE.to_string(),
        ]),
        code_challenge_methods_supported: Some(vec![PKCE_METHOD_S256.to_string()]),
    }
}

pub fn openid_configuration(
    issuer: String,
    base_url: &str,
    signing_algorithm: jwt::Algorithm,
) -> OpenIdConfiguration {
    OpenIdConfiguration {
        issuer,
        authorization_endpoint: endpoint_url(base_url, "/oauth/authorize"),
        token_endpoint: endpoint_url(base_url, "/oauth/token"),
        userinfo_endpoint: Some(endpoint_url(base_url, "/oauth/userinfo")),
        jwks_uri: endpoint_url(base_url, "/.well-known/jwks.json"),
        response_types_supported: vec![RESPONSE_CODE.to_string()],
        grant_types_supported: vec![
            GRANT_AUTHORIZATION_CODE.to_string(),
            GRANT_CLIENT_CREDENTIALS.to_string(),
            GRANT_REFRESH_TOKEN.to_string(),
        ],
        subject_types_supported: vec!["public".to_string()],
        id_token_signing_alg_values_supported: jwt_algorithm_name(signing_algorithm)
            .map(str::to_string)
            .into_iter()
            .collect(),
        scopes_supported: vec![
            SCOPE_OPENID.to_string(),
            SCOPE_EMAIL.to_string(),
            SCOPE_PROFILE.to_string(),
            SCOPE_OFFLINE_ACCESS.to_string(),
        ],
        claims_supported: vec![
            "sub".to_string(),
            "aud".to_string(),
            "azp".to_string(),
            "iat".to_string(),
            "exp".to_string(),
            "auth_time".to_string(),
            "nonce".to_string(),
            "email".to_string(),
            "email_verified".to_string(),
            "name".to_string(),
            "preferred_username".to_string(),
            "picture".to_string(),
        ],
        code_challenge_methods_supported: vec![PKCE_METHOD_S256.to_string()],
        token_endpoint_auth_methods_supported: vec![
            AUTH_METHOD_CLIENT_SECRET_BASIC.to_string(),
            AUTH_METHOD_CLIENT_SECRET_POST.to_string(),
            AUTH_METHOD_NONE.to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_base_url_prefers_explicit_base() {
        let base = endpoint_base_url(
            "http://issuer.example",
            Some("https://public.example/oauth"),
            "5050",
        );

        assert_eq!(base, "https://public.example/oauth");
    }
}
