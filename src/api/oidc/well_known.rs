use crate::err::ErrorResponse;
use crate::iam::auth::jwt::jwt_config;
use axum::Json;
use axum::response::IntoResponse;
use http::HeaderMap;
use http::header::CACHE_CONTROL;
use oidc::metadata::{Jwks, OAuthAuthorizationServerMetadata, OpenIdConfiguration};
use std::env;

fn jwks_cache_control() -> String {
    let max_age = env::var("JWKS_CACHE_MAX_AGE_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(300);
    format!("public, max-age={max_age}")
}

#[utoipa::path(
    get,
    path = "/.well-known/jwks.json",
    tags = ["Well Known"],
    responses(
        (status = 200, description = "OK", body = Jwks)
    )
)]
pub async fn get_jwks() -> Result<impl IntoResponse, ErrorResponse> {
    let jwks = jwt_config()
        .public_jwks()
        .map_err(ErrorResponse::internal)?;
    let jwks = oidc::metadata::jwks_response(jwks);

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
    tags = ["Well Known"],
    responses(
        (status = 200, description = "OK", body = OAuthAuthorizationServerMetadata)
    )
)]
pub async fn get_oauth_metadata() -> Result<Json<OAuthAuthorizationServerMetadata>, ErrorResponse> {
    let issuer = jwt::issuer_from_env();
    let base_url = crate::conf::public_base_url();

    Ok(Json(oidc::metadata::authorization_server_metadata(
        issuer, &base_url,
    )))
}

#[utoipa::path(
    get,
    path = "/.well-known/openid-configuration",
    tags = ["Well Known"],
    responses(
        (status = 200, description = "OK", body = OpenIdConfiguration)
    )
)]
pub async fn get_openid_configuration() -> Result<Json<OpenIdConfiguration>, ErrorResponse> {
    let issuer = jwt::issuer_from_env();
    let base_url = crate::conf::public_base_url();

    Ok(Json(oidc::metadata::openid_configuration(
        issuer,
        &base_url,
        jwt_config().id_token_algorithm(),
    )))
}
