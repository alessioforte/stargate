use crate::err::ErrorResponse;
use crate::etc::jwt::jwt_config;
use axum::Json;
use axum::response::IntoResponse;
use http::HeaderMap;
use http::header::CACHE_CONTROL;
use jwt::{AlgorithmParameters, EllipticCurve, PublicKeyUse};
use serde::Serialize;
use std::env;
use utoipa::ToSchema;

const GRANT_CLIENT_CREDENTIALS: &str = "client_credentials";
const GRANT_AUTHORIZATION_CODE: &str = "authorization_code";
const GRANT_REFRESH_TOKEN: &str = "refresh_token";
const RESPONSE_CODE: &str = "code";
const AUTH_METHOD_CLIENT_SECRET_BASIC: &str = "client_secret_basic";
const AUTH_METHOD_CLIENT_SECRET_POST: &str = "client_secret_post";

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

fn endpoint_base_url(issuer: &str) -> String {
    env::var("OAUTH_BASE_URL")
        .ok()
        .filter(|base_url| !base_url.trim().is_empty())
        .or_else(|| issuer.contains("://").then(|| issuer.to_string()))
        .unwrap_or_else(|| {
            let port = env::var("PORT").unwrap_or_else(|_| "5050".to_string());
            format!("http://localhost:{port}")
        })
}

fn endpoint_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn jwks_cache_control() -> String {
    let max_age = env::var("JWKS_CACHE_MAX_AGE_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(300);
    format!("public, max-age={max_age}")
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

fn elliptic_curve_name(curve: &EllipticCurve) -> &'static str {
    match curve {
        EllipticCurve::P256 => "P-256",
        EllipticCurve::P384 => "P-384",
        EllipticCurve::P521 => "P-521",
        EllipticCurve::Ed25519 => "Ed25519",
    }
}

fn jwt_algorithm_name(algorithm: jwt::Algorithm) -> &'static str {
    match algorithm {
        jwt::Algorithm::HS256 => "HS256",
        jwt::Algorithm::HS384 => "HS384",
        jwt::Algorithm::HS512 => "HS512",
        jwt::Algorithm::ES256 => "ES256",
        jwt::Algorithm::ES384 => "ES384",
        jwt::Algorithm::RS256 => "RS256",
        jwt::Algorithm::RS384 => "RS384",
        jwt::Algorithm::RS512 => "RS512",
        jwt::Algorithm::PS256 => "PS256",
        jwt::Algorithm::PS384 => "PS384",
        jwt::Algorithm::PS512 => "PS512",
        jwt::Algorithm::EdDSA => "EdDSA",
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
            crv: Some(elliptic_curve_name(&params.curve).to_string()),
            x: Some(params.x),
            y: Some(params.y),
        }),
        AlgorithmParameters::OctetKey(_) | AlgorithmParameters::OctetKeyPair(_) => None,
    }
}

fn jwks_response(jwk_set: jwt::JwkSet) -> Jwks {
    let mut keys = Vec::with_capacity(jwk_set.keys.len());
    for jwk in jwk_set.keys {
        if let Some(key) = public_jwk_from_jwk(jwk) {
            keys.push(key);
        }
    }
    Jwks { keys }
}

#[utoipa::path(
    get,
    path = "/.well-known/jwks.json",
    responses(
        (status = 200, description = "OK", body = Jwks)
    )
)]
pub async fn get_jwks() -> Result<impl IntoResponse, ErrorResponse> {
    let jwks = jwt_config()
        .public_jwks()
        .map_err(ErrorResponse::internal)?;
    let jwks = jwks_response(jwks);

    let mut headers = HeaderMap::new();
    headers.insert(
        CACHE_CONTROL,
        jwks_cache_control()
            .parse()
            .map_err(ErrorResponse::internal)?,
    );

    Ok((headers, Json(jwks)))
}

#[utoipa::path(
    get,
    path = "/.well-known/oauth-authorization-server",
    responses(
        (status = 200, description = "OK", body = OAuthAuthorizationServerMetadata)
    )
)]
pub async fn get_oauth_metadata() -> Result<Json<OAuthAuthorizationServerMetadata>, ErrorResponse> {
    let issuer = jwt::issuer_from_env();
    let base_url = endpoint_base_url(&issuer);

    Ok(Json(OAuthAuthorizationServerMetadata {
        issuer,
        authorization_endpoint: Some(endpoint_url(&base_url, "/oauth/authorize")),
        token_endpoint: Some(endpoint_url(&base_url, "/oauth/token")),
        introspection_endpoint: endpoint_url(&base_url, "/oauth/introspect"),
        revocation_endpoint: endpoint_url(&base_url, "/oauth/revoke"),
        jwks_uri: endpoint_url(&base_url, "/.well-known/jwks.json"),
        response_types_supported: vec![RESPONSE_CODE.to_string()],
        grant_types_supported: vec![
            GRANT_AUTHORIZATION_CODE.to_string(),
            GRANT_CLIENT_CREDENTIALS.to_string(),
            GRANT_REFRESH_TOKEN.to_string(),
        ],
        token_endpoint_auth_methods_supported: Some(vec![
            AUTH_METHOD_CLIENT_SECRET_BASIC.to_string(),
            AUTH_METHOD_CLIENT_SECRET_POST.to_string(),
            "none".to_string(),
        ]),
        code_challenge_methods_supported: Some(vec!["S256".to_string()]),
    }))
}

#[utoipa::path(
    get,
    path = "/.well-known/openid-configuration",
    responses(
        (status = 200, description = "OK", body = OpenIdConfiguration)
    )
)]
pub async fn get_openid_configuration() -> Result<Json<OpenIdConfiguration>, ErrorResponse> {
    let issuer = jwt::issuer_from_env();
    let base_url = endpoint_base_url(&issuer);

    Ok(Json(OpenIdConfiguration {
        issuer,
        authorization_endpoint: endpoint_url(&base_url, "/oauth/authorize"),
        token_endpoint: endpoint_url(&base_url, "/oauth/token"),
        userinfo_endpoint: Some(endpoint_url(&base_url, "/oauth/userinfo")),
        jwks_uri: endpoint_url(&base_url, "/.well-known/jwks.json"),
        response_types_supported: vec![RESPONSE_CODE.to_string()],
        grant_types_supported: vec![
            GRANT_AUTHORIZATION_CODE.to_string(),
            GRANT_CLIENT_CREDENTIALS.to_string(),
            GRANT_REFRESH_TOKEN.to_string(),
        ],
        subject_types_supported: vec!["public".to_string()],
        id_token_signing_alg_values_supported: vec![
            jwt_algorithm_name(jwt_config().algorithm()).to_string(),
        ],
        scopes_supported: vec![
            "openid".to_string(),
            "email".to_string(),
            "profile".to_string(),
            "offline_access".to_string(),
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
        code_challenge_methods_supported: vec!["S256".to_string()],
        token_endpoint_auth_methods_supported: vec![
            AUTH_METHOD_CLIENT_SECRET_BASIC.to_string(),
            AUTH_METHOD_CLIENT_SECRET_POST.to_string(),
            "none".to_string(),
        ],
    }))
}
