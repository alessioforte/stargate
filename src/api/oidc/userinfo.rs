use super::shared::{SCOPE_EMAIL, SCOPE_OPENID, SCOPE_PROFILE, parse_space_delimited};
use crate::err::{ErrorResponse, HttpError};
use axum::Json;
use axum::extract::Request;
use http::header::AUTHORIZATION;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Serialize, utoipa::ToSchema, PartialEq)]
pub struct UserInfoResponse {
    sub: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preferred_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    picture: Option<String>,
}

fn bearer_unauthorized(message: impl Into<String>) -> ErrorResponse {
    let mut err = ErrorResponse::from(HttpError::Unauthorized(message.into()));
    err.insert_header("WWW-Authenticate", "Bearer");
    err
}

fn bearer_token(req: &Request) -> Result<String, ErrorResponse> {
    let Some(value) = req.headers().get(AUTHORIZATION) else {
        return Err(bearer_unauthorized("bearer token is required"));
    };
    let value = value
        .to_str()
        .map_err(|_| bearer_unauthorized("invalid authorization header"))?;
    let Some((scheme, token)) = value.split_once(' ') else {
        return Err(bearer_unauthorized("invalid authorization header"));
    };
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(bearer_unauthorized("bearer token is required"));
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(bearer_unauthorized("bearer token is required"));
    }
    Ok(token.to_string())
}

fn insufficient_scope() -> ErrorResponse {
    let mut err = ErrorResponse::from(HttpError::Forbidden("openid scope is required".to_string()));
    err.insert_header(
        "WWW-Authenticate",
        "Bearer error=\"insufficient_scope\", scope=\"openid\"",
    );
    err
}

fn scope_set(scope: Option<&str>) -> Result<HashSet<String>, ErrorResponse> {
    Ok(parse_space_delimited(scope, "scope")?
        .into_iter()
        .collect::<HashSet<_>>())
}

async fn active_user_claims(token: &str) -> Result<(jwt::Claims, HashSet<String>), ErrorResponse> {
    let claims = crate::etc::jwt::jwt_config()
        .validate_oauth_access_token(token, None)
        .map_err(|_| bearer_unauthorized("invalid token"))?;

    if claims.auth_time.is_none() {
        return Err(bearer_unauthorized("token does not represent a user"));
    }

    if crate::act::token_revocation::is_revoked(&claims)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Err(bearer_unauthorized("invalid token"));
    }

    let Some(client_id) = claims.azp.as_deref() else {
        return Err(bearer_unauthorized("invalid token"));
    };
    let Some(client) = crate::db::get_oauth_client_by_client_id(client_id)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Err(bearer_unauthorized("invalid token"));
    };
    if !client.enabled {
        return Err(bearer_unauthorized("invalid token"));
    }

    let scopes = scope_set(claims.scope.as_deref())?;
    if !scopes.contains(SCOPE_OPENID) {
        return Err(insufficient_scope());
    }

    Ok((claims, scopes))
}

fn userinfo_from_user(user: &db::ent::User, scopes: &HashSet<String>) -> UserInfoResponse {
    let include_email = scopes.contains(SCOPE_EMAIL);
    let include_profile = scopes.contains(SCOPE_PROFILE);
    let name = if include_profile {
        let given_name = user.given_name.clone().unwrap_or_default();
        let family_name = user.family_name.clone().unwrap_or_default();
        Some(crate::fun::format_name(&given_name, &family_name))
    } else {
        None
    };

    UserInfoResponse {
        sub: user.id.clone(),
        email: include_email.then(|| user.email.clone()),
        email_verified: include_email.then_some(true),
        name,
        preferred_username: include_profile.then(|| user.nickname.clone()),
        picture: include_profile.then(|| user.picture.clone()).flatten(),
    }
}

async fn handle_userinfo(req: Request) -> Result<Json<UserInfoResponse>, ErrorResponse> {
    let token = bearer_token(&req)?;
    let (claims, scopes) = active_user_claims(&token).await?;
    let Some(user_id) = claims
        .sub_id
        .as_deref()
        .filter(|user_id| !user_id.is_empty())
    else {
        return Err(bearer_unauthorized("invalid token"));
    };
    if claims.sub != user_id {
        return Err(bearer_unauthorized("invalid token"));
    }

    let Some(user) = crate::db::get_user_by_id(user_id)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Err(bearer_unauthorized("invalid token"));
    };

    Ok(Json(userinfo_from_user(&user, &scopes)))
}

#[utoipa::path(
    get,
    path = "/oauth/userinfo",
    tags = ["OAuth"],
    responses(
        (status = 200, description = "OK", body = UserInfoResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn get_userinfo(req: Request) -> Result<Json<UserInfoResponse>, ErrorResponse> {
    handle_userinfo(req).await
}

#[utoipa::path(
    post,
    path = "/oauth/userinfo",
    tags = ["OAuth"],
    responses(
        (status = 200, description = "OK", body = UserInfoResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn post_userinfo(req: Request) -> Result<Json<UserInfoResponse>, ErrorResponse> {
    handle_userinfo(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> db::ent::User {
        db::ent::User::new("alice@example.com".to_string(), "alice".to_string())
            .given_name(Some("Alice".to_string()))
            .family_name(Some("Example".to_string()))
            .picture(Some("https://app.example.com/alice.png".to_string()))
    }

    #[test]
    fn userinfo_filters_claims_by_scope() {
        let user = user();
        let scopes = HashSet::from([SCOPE_OPENID.to_string()]);
        let body = userinfo_from_user(&user, &scopes);

        assert_eq!(body.sub, user.id);
        assert!(body.email.is_none());
        assert!(body.name.is_none());
    }

    #[test]
    fn userinfo_includes_email_and_profile_claims_when_scoped() {
        let user = user();
        let scopes = HashSet::from([
            SCOPE_OPENID.to_string(),
            SCOPE_EMAIL.to_string(),
            SCOPE_PROFILE.to_string(),
        ]);
        let body = userinfo_from_user(&user, &scopes);

        assert_eq!(body.sub, user.id);
        assert_eq!(body.email.as_deref(), Some("alice@example.com"));
        assert_eq!(body.email_verified, Some(true));
        assert_eq!(body.name.as_deref(), Some("Alice Example"));
        assert_eq!(body.preferred_username.as_deref(), Some("alice"));
        assert_eq!(
            body.picture.as_deref(),
            Some("https://app.example.com/alice.png")
        );
    }
}
