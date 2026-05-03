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
    jwks_uri: String,
    response_types_supported: Vec<String>,
    grant_types_supported: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_endpoint_auth_methods_supported: Option<Vec<String>>,
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
        authorization_endpoint: None,
        token_endpoint: None,
        jwks_uri: endpoint_url(&base_url, "/.well-known/jwks.json"),
        response_types_supported: Vec::new(),
        grant_types_supported: Vec::new(),
        token_endpoint_auth_methods_supported: None,
    }))
}
