use super::authorization_codes::{self, AuthorizationCodeRecord};
use super::shared::{
    AUTHORIZATION_CODE_TTL_SECS, GRANT_AUTHORIZATION_CODE, GRANT_REFRESH_TOKEN, PKCE_METHOD_S256,
    RESPONSE_CODE, SCOPE_OFFLINE_ACCESS, SCOPE_OPENID, oauth_bad_request, resolve_audience,
    resolve_scopes, scope_contains,
};
use crate::err::{ErrorResponse, HttpError};
use crate::etc::ext::RequestExt;
use axum::body::Body;
use axum::extract::{Query, Request};
use axum::http::StatusCode;
use axum::response::Response;
use chrono::{Duration, TimeZone, Utc};
use http::header::LOCATION;
use serde::Deserialize;
use std::collections::HashSet;
use store::Store;
use url::Url;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct AuthorizeQuery {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    scope: String,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: String,
    code_challenge_method: String,
    audience: Option<String>,
    prompt: Option<String>,
    max_age: Option<u64>,
}

struct AuthorizedUser {
    user: db::ent::User,
    auth_time: chrono::DateTime<Utc>,
}

#[derive(Default)]
struct AuthorizePrompt {
    none: bool,
    login: bool,
    consent: bool,
}

#[utoipa::path(
    get,
    path = "/oauth/authorize",
    tags = ["OAuth"],
    responses(
        (status = 302, description = "Redirects to the registered redirect_uri with an authorization code or OAuth error"),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn get_authorize(req: Request) -> Result<Response, ErrorResponse> {
    let token = req.get_token();
    let uri = req.uri().clone();
    drop(req);

    let Query(query): Query<AuthorizeQuery> = Query::try_from_uri(&uri)
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;

    let client = authorization_client(&query).await?;
    let redirect_uri = query.redirect_uri.as_str();
    let state = query.state.as_deref();

    let prompt = match validate_authorize_prompt(query.prompt.as_deref()) {
        Ok(prompt) => prompt,
        Err(err) => {
            return oauth_error_redirect(
                redirect_uri,
                "invalid_request",
                err.message.as_str(),
                state,
            );
        }
    };

    let (scopes, audience) = match validate_authorize_request(&client, &query) {
        Ok(value) => value,
        Err(err) => {
            return oauth_error_redirect(
                redirect_uri,
                "invalid_request",
                err.message.as_str(),
                state,
            );
        }
    };

    let Some(user) = current_authorized_user(token).await? else {
        return oauth_error_redirect(redirect_uri, "login_required", "login required", state);
    };

    if prompt.login {
        return oauth_error_redirect(
            redirect_uri,
            "login_required",
            "fresh login required",
            state,
        );
    }

    if let Some(max_age) = query.max_age {
        let auth_age = Utc::now().signed_duration_since(user.auth_time);
        if auth_age > Duration::seconds(max_age as i64) {
            return oauth_error_redirect(
                redirect_uri,
                "login_required",
                "fresh login required",
                state,
            );
        }
    }

    if !consent_satisfied(&client, &user.user, &scopes, audience.as_deref(), &prompt).await? {
        return oauth_error_redirect(redirect_uri, "consent_required", "consent required", state);
    }

    let code = store_authorization_code(&client, &query, &user, &scopes, audience).await?;
    oauth_code_redirect(redirect_uri, &code, state)
}

fn redirect_location(redirect_uri: &str, params: &[(&str, &str)]) -> Result<String, ErrorResponse> {
    let mut url = Url::parse(redirect_uri)
        .map_err(|_| oauth_bad_request("redirect_uri must be an absolute URI"))?;
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in params {
            query.append_pair(key, value);
        }
    }
    Ok(url.to_string())
}

fn found_redirect(location: String) -> Result<Response, ErrorResponse> {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(LOCATION, location)
        .body(Body::empty())
        .map_err(ErrorResponse::internal)
}

fn oauth_code_redirect(
    redirect_uri: &str,
    code: &str,
    state: Option<&str>,
) -> Result<Response, ErrorResponse> {
    let location = match state {
        Some(state) => redirect_location(redirect_uri, &[("code", code), ("state", state)])?,
        None => redirect_location(redirect_uri, &[("code", code)])?,
    };
    found_redirect(location)
}

fn oauth_error_redirect(
    redirect_uri: &str,
    error: &str,
    description: &str,
    state: Option<&str>,
) -> Result<Response, ErrorResponse> {
    let location = match state {
        Some(state) => redirect_location(
            redirect_uri,
            &[
                ("error", error),
                ("error_description", description),
                ("state", state),
            ],
        )?,
        None => redirect_location(
            redirect_uri,
            &[("error", error), ("error_description", description)],
        )?,
    };
    found_redirect(location)
}

fn exact_redirect_uri_allowed(client: &db::ent::OAuthClient, redirect_uri: &str) -> bool {
    client.redirect_uris.iter().any(|uri| uri == redirect_uri)
}

async fn authorization_client(
    query: &AuthorizeQuery,
) -> Result<db::ent::OAuthClient, ErrorResponse> {
    if query.client_id.trim().is_empty() {
        return Err(oauth_bad_request("client_id is required"));
    }
    if query.redirect_uri.trim().is_empty() {
        return Err(oauth_bad_request("redirect_uri is required"));
    }

    let Some(client) = crate::db::get_oauth_client_by_client_id(query.client_id.trim())
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Err(oauth_bad_request("unknown client_id"));
    };

    if !client.enabled {
        return Err(oauth_bad_request("client is disabled"));
    }

    if !exact_redirect_uri_allowed(&client, &query.redirect_uri) {
        return Err(oauth_bad_request(
            "redirect_uri is not registered for this client",
        ));
    }

    Ok(client)
}

fn validate_authorize_prompt(prompt: Option<&str>) -> Result<AuthorizePrompt, ErrorResponse> {
    let Some(prompt) = prompt else {
        return Ok(AuthorizePrompt::default());
    };

    let mut values = HashSet::new();
    for value in prompt.split_whitespace() {
        match value {
            "none" | "login" | "consent" => {
                values.insert(value.to_string());
            }
            _ => {
                return Err(oauth_bad_request("unsupported prompt value"));
            }
        }
    }

    if values.contains("none") && values.len() > 1 {
        return Err(oauth_bad_request(
            "prompt=none cannot be combined with other prompt values",
        ));
    }

    Ok(AuthorizePrompt {
        none: values.contains("none"),
        login: values.contains("login"),
        consent: values.contains("consent"),
    })
}

async fn current_authorized_user(
    token: Option<String>,
) -> Result<Option<AuthorizedUser>, ErrorResponse> {
    let Some(token) = token else {
        return Ok(None);
    };

    let claims = match crate::etc::jwt::jwt_config().validate_session_access_token(&token) {
        Ok(claims) => claims,
        _ => return Ok(None),
    };

    if crate::act::token_revocation::is_revoked(&claims)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Ok(None);
    }

    let Some(sid) = claims.sid.as_deref().filter(|sid| !sid.is_empty()) else {
        return Ok(None);
    };
    let Some(subject) = crate::etc::store::use_store()
        .get::<crate::etc::sub::Subject>(sid)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Ok(None);
    };

    if subject.sub_type != crate::etc::sub::SubjectType::User {
        return Ok(None);
    }

    let user_id = claims.sub_id.as_deref().unwrap_or(subject.id.as_str());
    let Some(user) = crate::db::get_user_by_id(user_id)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Ok(None);
    };

    let auth_time = claims.auth_time.unwrap_or(claims.iat);
    let auth_time = Utc
        .timestamp_opt(auth_time as i64, 0)
        .single()
        .unwrap_or_else(Utc::now);

    Ok(Some(AuthorizedUser { user, auth_time }))
}

fn validate_authorize_request(
    client: &db::ent::OAuthClient,
    query: &AuthorizeQuery,
) -> Result<(Vec<String>, Option<String>), ErrorResponse> {
    if query.response_type.trim() != RESPONSE_CODE {
        return Err(oauth_bad_request("unsupported response_type"));
    }

    if !client
        .grant_types
        .iter()
        .any(|grant_type| grant_type == GRANT_AUTHORIZATION_CODE)
    {
        return Err(oauth_bad_request(
            "authorization_code grant is not allowed for this client",
        ));
    }

    if !client
        .response_types
        .iter()
        .any(|response_type| response_type == RESPONSE_CODE)
    {
        return Err(oauth_bad_request(
            "code response type is not allowed for this client",
        ));
    }

    if query.code_challenge.trim().is_empty() {
        return Err(oauth_bad_request("code_challenge is required"));
    }

    if query.code_challenge_method.trim() != PKCE_METHOD_S256 {
        return Err(oauth_bad_request("code_challenge_method must be S256"));
    }

    let scopes = resolve_scopes(Some(query.scope.as_str()), &client.scopes)?;
    if !scope_contains(&scopes, SCOPE_OPENID) {
        return Err(oauth_bad_request("openid scope is required"));
    }

    if scope_contains(&scopes, SCOPE_OFFLINE_ACCESS)
        && !client
            .grant_types
            .iter()
            .any(|grant_type| grant_type == GRANT_REFRESH_TOKEN)
    {
        return Err(oauth_bad_request(
            "offline_access requires refresh_token grant",
        ));
    }

    let audience = resolve_audience(query.audience.as_deref(), &client.audiences)?;
    Ok((scopes, audience))
}

async fn consent_satisfied(
    client: &db::ent::OAuthClient,
    user: &db::ent::User,
    scopes: &[String],
    audience: Option<&str>,
    prompt: &AuthorizePrompt,
) -> Result<bool, ErrorResponse> {
    if oidc::consent::client_is_first_party(&client.attrs) {
        return Ok(true);
    }

    if prompt.consent {
        return Ok(false);
    }

    let Some(consent) = crate::db::get_active_oauth_consent(&user.id, &client.client_id)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Ok(false);
    };

    if prompt.none
        && !oidc::consent::consent_covers(&consent.scopes, &consent.audiences, scopes, audience)
    {
        return Ok(false);
    }

    Ok(oidc::consent::consent_covers(
        &consent.scopes,
        &consent.audiences,
        scopes,
        audience,
    ))
}

async fn store_authorization_code(
    client: &db::ent::OAuthClient,
    query: &AuthorizeQuery,
    user: &AuthorizedUser,
    scopes: &[String],
    audience: Option<String>,
) -> Result<String, ErrorResponse> {
    let code = oidc::codes::authorization_code();
    let code_hash = pw::hash_api_key(&code);
    let record = AuthorizationCodeRecord {
        client_id: client.client_id.clone(),
        user_id: user.user.id.clone(),
        redirect_uri: query.redirect_uri.clone(),
        scope: scopes.join(" "),
        audience,
        nonce: query.nonce.clone(),
        code_challenge: query.code_challenge.trim().to_string(),
        code_challenge_method: PKCE_METHOD_S256.to_string(),
        auth_time: user.auth_time,
    };

    authorization_codes::store(&code_hash, record, AUTHORIZATION_CODE_TTL_SECS as u64).await?;

    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::super::shared::AUTH_METHOD_NONE;
    use super::super::shared::{SCOPE_EMAIL, SCOPE_PROFILE};
    use super::*;

    fn authorization_code_client(client_id: &str) -> db::ent::OAuthClient {
        db::ent::OAuthClient::new(
            client_id.to_string(),
            None,
            "OIDC Client".to_string(),
            None,
            None,
            None,
            AUTH_METHOD_NONE.to_string(),
            vec![GRANT_AUTHORIZATION_CODE.to_string()],
            vec![RESPONSE_CODE.to_string()],
            vec!["https://app.example.com/callback".to_string()],
            vec![
                SCOPE_OPENID.to_string(),
                SCOPE_EMAIL.to_string(),
                SCOPE_PROFILE.to_string(),
            ],
            vec!["gateway".to_string()],
            serde_json::json!({}),
        )
    }

    #[test]
    fn authorize_request_requires_openid_scope() {
        let client = authorization_code_client("client-1");
        let query = AuthorizeQuery {
            response_type: RESPONSE_CODE.to_string(),
            client_id: "client-1".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: SCOPE_EMAIL.to_string(),
            state: None,
            nonce: None,
            code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string(),
            code_challenge_method: PKCE_METHOD_S256.to_string(),
            audience: None,
            prompt: None,
            max_age: None,
        };

        assert!(validate_authorize_request(&client, &query).is_err());
    }

    #[test]
    fn authorize_request_accepts_code_pkce_and_registered_scope() {
        let client = authorization_code_client("client-1");
        let query = AuthorizeQuery {
            response_type: RESPONSE_CODE.to_string(),
            client_id: "client-1".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: format!("{SCOPE_OPENID} {SCOPE_EMAIL}"),
            state: None,
            nonce: None,
            code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string(),
            code_challenge_method: PKCE_METHOD_S256.to_string(),
            audience: None,
            prompt: None,
            max_age: None,
        };

        let (scopes, audience) = validate_authorize_request(&client, &query).unwrap();

        assert_eq!(
            scopes,
            vec![SCOPE_OPENID.to_string(), SCOPE_EMAIL.to_string()]
        );
        assert_eq!(audience.as_deref(), Some("gateway"));
    }

    #[test]
    fn authorize_request_rejects_offline_access_without_refresh_grant() {
        let mut client = authorization_code_client("client-1");
        client.scopes.0.push(SCOPE_OFFLINE_ACCESS.to_string());
        let query = AuthorizeQuery {
            response_type: RESPONSE_CODE.to_string(),
            client_id: "client-1".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: format!("{SCOPE_OPENID} {SCOPE_OFFLINE_ACCESS}"),
            state: None,
            nonce: None,
            code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string(),
            code_challenge_method: PKCE_METHOD_S256.to_string(),
            audience: None,
            prompt: None,
            max_age: None,
        };

        assert!(validate_authorize_request(&client, &query).is_err());
    }

    #[tokio::test]
    async fn first_party_client_skips_consent() {
        let mut client = authorization_code_client("client-1");
        client.attrs = serde_json::json!({ "first_party": true });
        let user = db::ent::User::new("alice@example.com".to_string(), "alice".to_string());
        let scopes = vec![SCOPE_OPENID.to_string()];
        let prompt = AuthorizePrompt::default();

        let satisfied = consent_satisfied(&client, &user, &scopes, Some("gateway"), &prompt)
            .await
            .unwrap();

        assert!(satisfied);
    }
}
