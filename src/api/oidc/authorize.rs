use super::authorization_codes::{self, AuthorizationCodeRecord};
use super::interactions;
use super::shared::{
    AUTH_METHOD_NONE, AUTHORIZATION_CODE_TTL_SECS, GRANT_AUTHORIZATION_CODE, GRANT_REFRESH_TOKEN,
    OAuthResult, PKCE_METHOD_S256, RESPONSE_CODE, SCOPE_OFFLINE_ACCESS, SCOPE_OPENID,
    oauth_bad_request, parse_space_delimited, resolve_audience, scope_contains,
};
use crate::err::OAuthErrorResponse;
use crate::etc::http::request::RequestExt;
use axum::body::Body;
use axum::extract::{Form, FromRequest, Query, Request};
use axum::http::StatusCode;
use axum::response::Response;
use chrono::{TimeZone, Utc};
use http::header::{LOCATION, SET_COOKIE};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use store::Store;
use url::Url;

/// Authorization request parameters. Everything is optional so that a
/// missing or malformed value can be reported to the client's redirect_uri
/// (OIDC Core 3.1.2.6) instead of failing before the client is known.
/// Unrecognized parameters (`display`, `ui_locales`, `acr_values`, ...) are
/// ignored as OIDC Core requires.
#[derive(Clone, Debug, Default, Serialize, Deserialize, utoipa::IntoParams)]
pub struct AuthorizeQuery {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<String>,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    audience: Option<String>,
    prompt: Option<String>,
    max_age: Option<String>,
    pub(crate) login_hint: Option<String>,
    request: Option<String>,
    request_uri: Option<String>,
    /// Resumes a request parked while the user signed in.
    interaction: Option<String>,
}

impl AuthorizeQuery {
    pub(crate) fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    pub(crate) fn redirect_uri(&self) -> Option<&str> {
        self.redirect_uri.as_deref()
    }

    pub(crate) fn state(&self) -> Option<&str> {
        self.state.as_deref()
    }
}

struct AuthorizedUser {
    user: db::ent::User,
    sid: String,
    auth_time: chrono::DateTime<Utc>,
}

#[derive(Default)]
struct AuthorizePrompt {
    none: bool,
    login: bool,
    consent: bool,
}

struct ValidatedAuthorizeRequest {
    scopes: Vec<String>,
    audience: Option<String>,
    code_challenge: Option<String>,
    max_age: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/oauth/authorize",
    tags = ["OAuth"],
    params(AuthorizeQuery),
    responses(
        (status = 302, description = "Redirects to the registered redirect_uri with an authorization code or OAuth error, or to the provider login page"),
        (status = 400, description = "Bad Request", body = OAuthErrorResponse),
        (status = 500, description = "Internal Server Error", body = OAuthErrorResponse),
    )
)]
pub async fn get_authorize(req: Request) -> OAuthResult<Response> {
    let token = req.get_token();
    let Query(query) = Query::<AuthorizeQuery>::try_from_uri(req.uri())
        .map_err(|e| oauth_bad_request(e.body_text()))?;
    authorize(token, query).await
}

#[utoipa::path(
    post,
    path = "/oauth/authorize",
    tags = ["OAuth"],
    request_body(content = String, content_type = "application/x-www-form-urlencoded", description = "Authorization request parameters (OIDC Core 3.1.2.1)"),
    responses(
        (status = 302, description = "Redirects to the registered redirect_uri with an authorization code or OAuth error, or to the provider login page"),
        (status = 400, description = "Bad Request", body = OAuthErrorResponse),
        (status = 500, description = "Internal Server Error", body = OAuthErrorResponse),
    )
)]
pub async fn post_authorize(req: Request) -> OAuthResult<Response> {
    let token = req.get_token();
    let Form(query) = Form::<AuthorizeQuery>::from_request(req, &())
        .await
        .map_err(|e| oauth_bad_request(e.body_text()))?;
    authorize(token, query).await
}

async fn authorize(token: Option<String>, query: AuthorizeQuery) -> OAuthResult<Response> {
    // A resumed request replays the parked parameters; the rest of this
    // request's query is ignored.
    let (query, parked_at, consent_stage) = match query.interaction.as_deref() {
        Some(id) => {
            let Some(pending) = interactions::take(id).await? else {
                return Err(oauth_bad_request(
                    "authorization request expired or was already completed",
                ));
            };
            (pending.query, Some(pending.created_at), pending.consent)
        }
        None => (query, None, None),
    };

    let client = authorization_client(&query).await?;
    let redirect_uri = query.redirect_uri.as_deref().unwrap_or_default();
    let state = query.state.as_deref();
    let error_redirect = |error: &str, description: &str| {
        oauth_error_redirect(redirect_uri, error, description, state)
    };

    if query.request.is_some() {
        return error_redirect("request_not_supported", "request objects are not supported");
    }
    if query.request_uri.is_some() {
        return error_redirect("request_uri_not_supported", "request_uri is not supported");
    }

    let prompt = match validate_authorize_prompt(query.prompt.as_deref()) {
        Ok(prompt) => prompt,
        Err(err) => return error_redirect(&err.error, err.description()),
    };

    let request = match validate_authorize_request(&client, &query) {
        Ok(request) => request,
        Err(err) => return error_redirect(&err.error, err.description()),
    };

    let user = current_authorized_user(token).await?;
    // A parked consent stage only counts for the user it was created for.
    let stage_for_user = |user: &AuthorizedUser| {
        consent_stage
            .as_ref()
            .filter(|stage| stage.user_id == user.user.id)
    };
    let user = match user {
        // Parked for consent after this user passed the login checks.
        Some(user) if stage_for_user(&user).is_some() => user,
        // Signed in on the login page for this very request.
        Some(user) if parked_at.is_some_and(|at| user.auth_time.timestamp() >= at) => user,
        Some(user) if !prompt.login && !max_age_exceeded(&user, request.max_age) => user,
        _ if prompt.none => return error_redirect("login_required", "login required"),
        _ => return login_redirect(query).await,
    };

    let approved = stage_for_user(&user).is_some_and(|stage| stage.approved);
    if !approved
        && !consent_satisfied(
            &client,
            &user.user,
            &request.scopes,
            request.audience.as_deref(),
            &prompt,
        )
        .await?
    {
        if prompt.none {
            return error_redirect("consent_required", "consent required");
        }
        return consent_redirect(
            query,
            interactions::ConsentStage {
                user_id: user.user.id.clone(),
                scopes: request.scopes.clone(),
                audience: request.audience.clone(),
                approved: false,
            },
        )
        .await;
    }

    let code = store_authorization_code(&client, &query, &user, &request).await?;
    oauth_code_redirect(redirect_uri, &code, state)
}

fn max_age_exceeded(user: &AuthorizedUser, max_age: Option<i64>) -> bool {
    let Some(max_age) = max_age else {
        return false;
    };
    // max_age=0 is equivalent to prompt=login (OIDC Core 3.1.2.1).
    max_age == 0 || Utc::now().timestamp() - user.auth_time.timestamp() > max_age
}

/// Park the request and send the browser to the hosted auth app's login
/// page, which resumes it after a successful sign-in.
async fn login_redirect(mut query: AuthorizeQuery) -> OAuthResult<Response> {
    query.interaction = None;
    let login_hint = query.login_hint.clone();
    let id = interactions::park(query).await?;
    let mut response = found_redirect(interactions::login_page_url(interactions::LoginPage {
        interaction: Some(&id),
        login_hint: login_hint.as_deref(),
        ..Default::default()
    }))?;
    let cookie = interactions::interaction_cookie(&id, interactions::INTERACTION_TTL_SECS);
    response.headers_mut().append(
        SET_COOKIE,
        cookie.parse().map_err(OAuthErrorResponse::internal)?,
    );
    Ok(response)
}

/// Park the request for the signed-in user's consent and send the browser to
/// the hosted auth app's consent page, which resumes it after the decision.
async fn consent_redirect(
    mut query: AuthorizeQuery,
    stage: interactions::ConsentStage,
) -> OAuthResult<Response> {
    query.interaction = None;
    let id = interactions::park_for_consent(query, stage).await?;
    found_redirect(interactions::consent_page_url(&id))
}

fn redirect_location(redirect_uri: &str, params: &[(&str, &str)]) -> OAuthResult<String> {
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

fn found_redirect(location: String) -> OAuthResult<Response> {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(LOCATION, location)
        .body(Body::empty())
        .map_err(OAuthErrorResponse::internal)
}

fn oauth_code_redirect(
    redirect_uri: &str,
    code: &str,
    state: Option<&str>,
) -> OAuthResult<Response> {
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
) -> OAuthResult<Response> {
    found_redirect(error_redirect_location(
        redirect_uri,
        error,
        description,
        state,
    )?)
}

/// `redirect_uri` with an OAuth error response (RFC 6749 §4.1.2.1).
pub(super) fn error_redirect_location(
    redirect_uri: &str,
    error: &str,
    description: &str,
    state: Option<&str>,
) -> OAuthResult<String> {
    match state {
        Some(state) => redirect_location(
            redirect_uri,
            &[
                ("error", error),
                ("error_description", description),
                ("state", state),
            ],
        ),
        None => redirect_location(
            redirect_uri,
            &[("error", error), ("error_description", description)],
        ),
    }
}

fn exact_redirect_uri_allowed(client: &db::ent::OAuthClient, redirect_uri: &str) -> bool {
    client.redirect_uris.iter().any(|uri| uri == redirect_uri)
}

fn oauth_error(error: &str, description: impl Into<String>) -> OAuthErrorResponse {
    let mut err = oauth_bad_request(description);
    err.error = error.to_string();
    err
}

async fn authorization_client(query: &AuthorizeQuery) -> OAuthResult<db::ent::OAuthClient> {
    let client_id = query
        .client_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if client_id.is_empty() {
        return Err(oauth_bad_request("client_id is required"));
    }
    let redirect_uri = query.redirect_uri.as_deref().unwrap_or_default();
    if redirect_uri.trim().is_empty() {
        return Err(oauth_bad_request("redirect_uri is required"));
    }

    let Some(client) = crate::db::get_oauth_client_by_client_id(client_id)
        .await
        .map_err(OAuthErrorResponse::internal)?
    else {
        return Err(oauth_bad_request("unknown client_id"));
    };

    if !client.enabled {
        return Err(oauth_bad_request("client is disabled"));
    }

    if !exact_redirect_uri_allowed(&client, redirect_uri) {
        return Err(oauth_bad_request(
            "redirect_uri is not registered for this client",
        ));
    }

    Ok(client)
}

fn validate_authorize_prompt(prompt: Option<&str>) -> OAuthResult<AuthorizePrompt> {
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
                return Err(oauth_bad_request(format!(
                    "unsupported prompt value '{value}'"
                )));
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

async fn current_authorized_user(token: Option<String>) -> OAuthResult<Option<AuthorizedUser>> {
    let Some(token) = token else {
        return Ok(None);
    };

    let claims = match crate::etc::auth::jwt::jwt_config().validate_session_access_token(&token) {
        Ok(claims) => claims,
        _ => return Ok(None),
    };

    if crate::act::token_revocation::is_revoked(&claims)
        .await
        .map_err(OAuthErrorResponse::internal)?
    {
        return Ok(None);
    }

    let Some(sid) = claims.sid.as_deref().filter(|sid| !sid.is_empty()) else {
        return Ok(None);
    };
    let Some(subject) = crate::etc::store::use_store()
        .get::<crate::etc::auth::subject::Subject>(sid)
        .await
        .map_err(OAuthErrorResponse::internal)?
    else {
        return Ok(None);
    };

    if subject.sub_type != crate::etc::auth::subject::SubjectType::User {
        return Ok(None);
    }

    let user_id = claims.sub_id.as_deref().unwrap_or(subject.id.as_str());
    let Some(user) = crate::db::get_user_by_id(user_id)
        .await
        .map_err(OAuthErrorResponse::internal)?
    else {
        return Ok(None);
    };

    let Some(auth_time) = claims.auth_time else {
        return Ok(None);
    };
    let auth_time = Utc
        .timestamp_opt(auth_time as i64, 0)
        .single()
        .unwrap_or_else(Utc::now);

    Ok(Some(AuthorizedUser {
        user,
        sid: sid.to_string(),
        auth_time,
    }))
}

fn validate_authorize_request(
    client: &db::ent::OAuthClient,
    query: &AuthorizeQuery,
) -> OAuthResult<ValidatedAuthorizeRequest> {
    let response_type = query
        .response_type
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if response_type.is_empty() {
        return Err(oauth_bad_request("response_type is required"));
    }
    if response_type != RESPONSE_CODE {
        return Err(oauth_error(
            "unsupported_response_type",
            "only the code response type is supported",
        ));
    }

    if !client
        .grant_types
        .iter()
        .any(|grant_type| grant_type == GRANT_AUTHORIZATION_CODE)
        || !client
            .response_types
            .iter()
            .any(|response_type| response_type == RESPONSE_CODE)
    {
        return Err(oauth_error(
            "unauthorized_client",
            "authorization_code grant is not allowed for this client",
        ));
    }

    let code_challenge = validate_code_challenge(client, query)?;

    // Scopes the client is not registered for are dropped rather than
    // rejected, as OIDC Core 3.1.2.1 asks servers to ignore unknown scopes.
    let requested = parse_space_delimited(query.scope.as_deref(), "scope")?;
    if requested.is_empty() {
        return Err(oauth_bad_request("scope is required"));
    }
    if !scope_contains(&requested, SCOPE_OPENID) {
        return Err(oauth_error("invalid_scope", "openid scope is required"));
    }
    let scopes = requested
        .into_iter()
        .filter(|scope| client.scopes.iter().any(|allowed| allowed == scope))
        .collect::<Vec<_>>();
    if !scope_contains(&scopes, SCOPE_OPENID) {
        return Err(oauth_error(
            "invalid_scope",
            "openid scope is not allowed for this client",
        ));
    }

    if scope_contains(&scopes, SCOPE_OFFLINE_ACCESS)
        && !client
            .grant_types
            .iter()
            .any(|grant_type| grant_type == GRANT_REFRESH_TOKEN)
    {
        return Err(oauth_error(
            "invalid_scope",
            "offline_access requires refresh_token grant",
        ));
    }

    let max_age = match query.max_age.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(value) => Some(
            value
                .parse::<u32>()
                .map_err(|_| oauth_bad_request("max_age must be a non-negative integer"))?
                as i64,
        ),
    };

    let audience = resolve_audience(query.audience.as_deref(), &client.audiences)?;
    Ok(ValidatedAuthorizeRequest {
        scopes,
        audience,
        code_challenge,
        max_age,
    })
}

fn validate_code_challenge(
    client: &db::ent::OAuthClient,
    query: &AuthorizeQuery,
) -> OAuthResult<Option<String>> {
    let challenge = query
        .code_challenge
        .as_deref()
        .map(str::trim)
        .filter(|challenge| !challenge.is_empty());
    let Some(challenge) = challenge else {
        let public_client = client.token_endpoint_auth_method == AUTH_METHOD_NONE;
        if oidc::pkce::client_requires_pkce(public_client, &client.attrs) {
            return Err(oauth_bad_request("code_challenge is required"));
        }
        return Ok(None);
    };

    if query.code_challenge_method.as_deref().map(str::trim) != Some(PKCE_METHOD_S256) {
        return Err(oauth_bad_request("code_challenge_method must be S256"));
    }

    Ok(Some(challenge.to_string()))
}

async fn consent_satisfied(
    client: &db::ent::OAuthClient,
    user: &db::ent::User,
    scopes: &[String],
    audience: Option<&str>,
    prompt: &AuthorizePrompt,
) -> OAuthResult<bool> {
    if oidc::consent::client_is_first_party(&client.attrs) {
        return Ok(true);
    }

    if prompt.consent {
        return Ok(false);
    }

    let Some(consent) = crate::db::get_active_oauth_consent(&user.id, &client.client_id)
        .await
        .map_err(OAuthErrorResponse::internal)?
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
    request: &ValidatedAuthorizeRequest,
) -> OAuthResult<String> {
    let code = oidc::codes::authorization_code();
    let code_hash = pw::hash_api_key(&code);
    let record = AuthorizationCodeRecord {
        client_id: client.client_id.clone(),
        user_id: user.user.id.clone(),
        sid: Some(user.sid.clone()),
        redirect_uri: query.redirect_uri.clone().unwrap_or_default(),
        scope: request.scopes.join(" "),
        audience: request.audience.clone(),
        nonce: query.nonce.clone(),
        // Empty when the client is allowed to skip PKCE.
        code_challenge: request.code_challenge.clone().unwrap_or_default(),
        code_challenge_method: request
            .code_challenge
            .as_ref()
            .map(|_| PKCE_METHOD_S256.to_string())
            .unwrap_or_default(),
        auth_time: user.auth_time,
    };

    authorization_codes::store(&code_hash, record, AUTHORIZATION_CODE_TTL_SECS as u64).await?;

    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::super::shared::{AUTH_METHOD_CLIENT_SECRET_BASIC, SCOPE_EMAIL, SCOPE_PROFILE};
    use super::*;

    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    fn authorization_code_client(client_id: &str) -> db::ent::OAuthClient {
        db::ent::OAuthClient::new(
            client_id.to_string(),
            None,
            "OIDC Client".to_string(),
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

    fn query(scope: &str) -> AuthorizeQuery {
        AuthorizeQuery {
            response_type: Some(RESPONSE_CODE.to_string()),
            client_id: Some("client-1".to_string()),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            scope: Some(scope.to_string()),
            code_challenge: Some(CHALLENGE.to_string()),
            code_challenge_method: Some(PKCE_METHOD_S256.to_string()),
            ..AuthorizeQuery::default()
        }
    }

    fn error_code(result: OAuthResult<ValidatedAuthorizeRequest>) -> String {
        match result {
            Ok(_) => panic!("expected an error"),
            Err(err) => err.error,
        }
    }

    #[test]
    fn authorize_request_requires_openid_scope() {
        let client = authorization_code_client("client-1");

        assert_eq!(
            error_code(validate_authorize_request(&client, &query(SCOPE_EMAIL))),
            "invalid_scope"
        );
    }

    #[test]
    fn authorize_request_accepts_code_pkce_and_registered_scope() {
        let client = authorization_code_client("client-1");
        let request =
            validate_authorize_request(&client, &query(&format!("{SCOPE_OPENID} {SCOPE_EMAIL}")))
                .unwrap();

        assert_eq!(
            request.scopes,
            vec![SCOPE_OPENID.to_string(), SCOPE_EMAIL.to_string()]
        );
        assert_eq!(request.audience.as_deref(), Some("gateway"));
        assert_eq!(request.code_challenge.as_deref(), Some(CHALLENGE));
    }

    #[test]
    fn authorize_request_ignores_unregistered_scopes() {
        let client = authorization_code_client("client-1");
        let request = validate_authorize_request(&client, &query("openid address phone")).unwrap();

        assert_eq!(request.scopes, vec![SCOPE_OPENID.to_string()]);
    }

    #[test]
    fn authorize_request_reports_response_type_errors() {
        let client = authorization_code_client("client-1");
        let mut missing = query(SCOPE_OPENID);
        missing.response_type = None;
        let mut token = query(SCOPE_OPENID);
        token.response_type = Some("token".to_string());

        assert_eq!(
            error_code(validate_authorize_request(&client, &missing)),
            "invalid_request"
        );
        assert_eq!(
            error_code(validate_authorize_request(&client, &token)),
            "unsupported_response_type"
        );
    }

    #[test]
    fn pkce_is_required_unless_confidential_client_opts_out() {
        let mut client = authorization_code_client("client-1");
        let mut without_pkce = query(SCOPE_OPENID);
        without_pkce.code_challenge = None;
        without_pkce.code_challenge_method = None;

        assert!(validate_authorize_request(&client, &without_pkce).is_err());

        client.token_endpoint_auth_method = AUTH_METHOD_CLIENT_SECRET_BASIC.to_string();
        assert!(validate_authorize_request(&client, &without_pkce).is_err());

        client.attrs = serde_json::json!({ "require_pkce": false });
        let request = validate_authorize_request(&client, &without_pkce).unwrap();
        assert!(request.code_challenge.is_none());

        // A public client cannot opt out.
        client.token_endpoint_auth_method = AUTH_METHOD_NONE.to_string();
        assert!(validate_authorize_request(&client, &without_pkce).is_err());
    }

    #[test]
    fn authorize_request_rejects_invalid_max_age() {
        let client = authorization_code_client("client-1");
        let mut request = query(SCOPE_OPENID);
        request.max_age = Some("-1".to_string());

        assert!(validate_authorize_request(&client, &request).is_err());

        request.max_age = Some("10000".to_string());
        assert_eq!(
            validate_authorize_request(&client, &request)
                .unwrap()
                .max_age,
            Some(10000)
        );
    }

    #[test]
    fn authorize_request_rejects_offline_access_without_refresh_grant() {
        let mut client = authorization_code_client("client-1");
        client.scopes.0.push(SCOPE_OFFLINE_ACCESS.to_string());

        assert!(
            validate_authorize_request(
                &client,
                &query(&format!("{SCOPE_OPENID} {SCOPE_OFFLINE_ACCESS}"))
            )
            .is_err()
        );
    }

    #[test]
    fn max_age_zero_always_requires_login() {
        let user = AuthorizedUser {
            user: db::ent::User::new("alice@example.com".to_string(), "alice".to_string()),
            sid: "sid-1".to_string(),
            auth_time: Utc::now(),
        };

        assert!(max_age_exceeded(&user, Some(0)));
        assert!(!max_age_exceeded(&user, Some(60)));
        assert!(!max_age_exceeded(&user, None));
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
