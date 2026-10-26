use super::authorization_codes::{self, AuthorizationCodeRecord};
use super::interactions;
use super::shared::{
    AUTH_METHOD_NONE, AUTHORIZATION_CODE_TTL_SECS, GRANT_AUTHORIZATION_CODE, GRANT_REFRESH_TOKEN,
    OAuthResult, PKCE_METHOD_S256, RESPONSE_CODE, SCOPE_OFFLINE_ACCESS, SCOPE_OPENID,
    oauth_bad_request, parse_space_delimited, resolve_audience, scope_contains,
};
use crate::err::OAuthErrorResponse;
use crate::web::request::RequestExt;
use axum::body::Body;
use axum::extract::{Form, FromRequest, Query, Request};
use axum::http::StatusCode;
use axum::response::Response;
use chrono::{TimeZone, Utc};
use http::header::{CACHE_CONTROL, CONTENT_TYPE, LOCATION, PRAGMA, SET_COOKIE};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use store::Store;
use url::Url;

/// Authorization request parameters. Everything is optional so that a
/// missing or malformed value can be reported to the client's redirect_uri
/// (OIDC Core 3.1.2.6) instead of failing before the client is known.
/// Unrecognized parameters (`display`, `ui_locales`, ...) are ignored as OIDC
/// Core requires.
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
    /// A previously issued ID token naming the expected user (may be expired).
    id_token_hint: Option<String>,
    /// Requested assurance levels, most preferred first (voluntary).
    acr_values: Option<String>,
    /// `query` (default) or `form_post`.
    response_mode: Option<String>,
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
}

/// The browser's signed-in user, from the session cookie.
pub(super) struct AuthorizedUser {
    pub(super) user: db::ent::User,
    pub(super) sid: String,
    auth_time: chrono::DateTime<Utc>,
    /// How the session authenticated (OIDC `amr`).
    amr: Vec<String>,
}

/// How a single-factor session can reach the multi-factor level.
enum StepUpRoute {
    /// Verify an email code on the hosted step-up page.
    EmailCode,
    /// Sign in with a password first (an email code alone cannot add a
    /// factor to an email-code session).
    SignIn,
    /// Not possible: the actual level is issued (`acr_values` is voluntary).
    Unavailable,
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
    let (query, parked_at, consent_stage, step_up_stage) = match query.interaction.as_deref() {
        Some(id) => {
            let Some(pending) = interactions::take(id).await? else {
                return Err(oauth_bad_request(
                    "authorization request expired or was already completed",
                ));
            };
            (
                pending.query,
                Some(pending.created_at),
                pending.consent,
                pending.step_up,
            )
        }
        None => (query, None, None, None),
    };

    let client = authorization_client(&query).await?;
    let redirect_uri = query.redirect_uri.as_deref().unwrap_or_default();
    // A request object is rejected below, in the response mode it asks for.
    let request_object = query
        .request
        .as_deref()
        .map(oidc::request_object::response_parameters)
        .unwrap_or_default();
    let state = request_object.state.as_deref().or(query.state.as_deref());
    let response_mode = request_object
        .response_mode
        .as_deref()
        .or(query.response_mode.as_deref());
    let Some(mode) = ResponseMode::parse(response_mode) else {
        return client_response(
            ResponseMode::Query,
            redirect_uri,
            &error_params("invalid_request", "unsupported response_mode", state),
        );
    };
    let error_redirect = |error: &str, description: &str| {
        client_response(mode, redirect_uri, &error_params(error, description, state))
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

    let hinted_subject = match query
        .id_token_hint
        .as_deref()
        .map(str::trim)
        .filter(|hint| !hint.is_empty())
    {
        Some(hint) => match crate::etc::auth::jwt::jwt_config().validate_id_token_hint(hint) {
            Ok(claims) => Some(claims.sub),
            Err(_) => return error_redirect("invalid_request", "id_token_hint is invalid"),
        },
        None => None,
    };

    let user = current_authorized_user(token).await?;
    // The session must belong to the user the hint names (OIDC Core 3.1.2.1).
    if let (Some(user), Some(subject)) = (&user, &hinted_subject)
        && user.user.id != *subject
    {
        if prompt.none || parked_at.is_some() {
            return error_redirect(
                "login_required",
                "the signed-in user is not the one named by id_token_hint",
            );
        }
        return login_redirect(query, hinted_subject.as_deref()).await;
    }
    // A parked consent stage only counts for the user it was created for.
    let stage_for_user = |user: &AuthorizedUser| {
        consent_stage
            .as_ref()
            .filter(|stage| stage.user_id == user.user.id)
    };
    let stepped_up = |user: &AuthorizedUser| {
        step_up_stage
            .as_ref()
            .is_some_and(|stage| stage.user_id == user.user.id)
    };
    let user = match user {
        // Parked for consent or step-up after this user passed the login checks.
        Some(user) if stage_for_user(&user).is_some() || stepped_up(&user) => user,
        // Signed in on the login page for this very request.
        Some(user) if parked_at.is_some_and(|at| user.auth_time.timestamp() >= at) => user,
        Some(user) if !prompt.login && !max_age_exceeded(&user, request.max_age) => user,
        _ if prompt.none => return error_redirect("login_required", "login required"),
        _ => return login_redirect(query, hinted_subject.as_deref()).await,
    };

    // The user declined on the consent page.
    if stage_for_user(&user)
        .is_some_and(|stage| stage.decision == Some(interactions::ConsentDecision::Denied))
    {
        return error_redirect("access_denied", "the user denied the request");
    }

    // Assurance level: `acr_values` asks for one (OIDC Core 3.1.2.1).
    let level = oidc::authentication::AssuranceLevel::of(&user.amr);
    let acr = match oidc::authentication::decide_acr(query.acr_values.as_deref(), level) {
        oidc::authentication::AcrDecision::Satisfied(acr) => acr,
        oidc::authentication::AcrDecision::StepUp => {
            match step_up_route(&user, parked_at.is_some()).await? {
                StepUpRoute::Unavailable => level.acr(),
                _ if prompt.none => {
                    return error_redirect(
                        "interaction_required",
                        "multi-factor authentication required",
                    );
                }
                StepUpRoute::EmailCode => return step_up_redirect(query, &user).await,
                StepUpRoute::SignIn => {
                    return login_redirect(query, hinted_subject.as_deref()).await;
                }
            }
        }
    };
    let authentication = oidc::authentication::Authentication {
        amr: user.amr.clone(),
        acr: acr.to_string(),
    };

    let approved = stage_for_user(&user)
        .is_some_and(|stage| stage.decision == Some(interactions::ConsentDecision::Approved));
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
                decision: None,
            },
        )
        .await;
    }

    let code = store_authorization_code(&client, &query, &user, &request, authentication).await?;
    let mut params = vec![("code", code.as_str())];
    params.extend(state.map(|state| ("state", state)));
    client_response(mode, redirect_uri, &params)
}

fn max_age_exceeded(user: &AuthorizedUser, max_age: Option<i64>) -> bool {
    let Some(max_age) = max_age else {
        return false;
    };
    // max_age=0 is equivalent to prompt=login (OIDC Core 3.1.2.1).
    max_age == 0 || Utc::now().timestamp() - user.auth_time.timestamp() > max_age
}

/// Park the request and send the browser to the hosted auth app's login
/// page, which resumes it after a successful sign-in. Without a `login_hint`,
/// the user named by `id_token_hint` is prefilled.
async fn login_redirect(
    mut query: AuthorizeQuery,
    hinted_subject: Option<&str>,
) -> OAuthResult<Response> {
    query.interaction = None;
    if query.login_hint.is_none()
        && let Some(subject) = hinted_subject
    {
        query.login_hint = crate::data::db::get_user_by_id(subject)
            .await
            .map_err(OAuthErrorResponse::internal)?
            .map(|user| user.email);
    }
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

/// How `user` can add a factor. `signed_in_for_request`: the request was
/// already parked, so the user has just signed in (or stepped up) for it.
async fn step_up_route(
    user: &AuthorizedUser,
    signed_in_for_request: bool,
) -> OAuthResult<StepUpRoute> {
    let available = crate::act::otp::service::email_mfa_available(&user.user)
        .await
        .map_err(|error| OAuthErrorResponse::internal(error.message))?;
    Ok(if !available {
        StepUpRoute::Unavailable
    } else if oidc::authentication::otp_adds_factor(&user.amr) {
        StepUpRoute::EmailCode
    } else if signed_in_for_request {
        // Signed in with an email code again: do not loop.
        StepUpRoute::Unavailable
    } else {
        StepUpRoute::SignIn
    })
}

/// Park the request until the signed-in user verifies an email code, and send
/// the browser to the hosted auth app's step-up page, which resumes it.
async fn step_up_redirect(
    mut query: AuthorizeQuery,
    user: &AuthorizedUser,
) -> OAuthResult<Response> {
    query.interaction = None;
    let id = interactions::park_for_step_up(
        query,
        interactions::StepUpStage {
            user_id: user.user.id.clone(),
        },
    )
    .await?;
    found_redirect(interactions::step_up_page_url(&id))
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

/// How the authorization response reaches the client: in the redirect's
/// query (default for `code`), or posted by an auto-submitting form (OAuth 2.0
/// Form Post Response Mode), which keeps the code out of URLs and logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResponseMode {
    Query,
    FormPost,
}

impl ResponseMode {
    /// `None` for an unsupported value (`fragment`, `jwt`, ...).
    fn parse(value: Option<&str>) -> Option<Self> {
        match value.map(str::trim).unwrap_or_default() {
            "" | "query" => Some(ResponseMode::Query),
            "form_post" => Some(ResponseMode::FormPost),
            _ => None,
        }
    }
}

/// Error response parameters (RFC 6749 §4.1.2.1).
fn error_params<'a>(
    error: &'a str,
    description: &'a str,
    state: Option<&'a str>,
) -> Vec<(&'a str, &'a str)> {
    let mut params = vec![("error", error), ("error_description", description)];
    params.extend(state.map(|state| ("state", state)));
    params
}

/// Deliver an authorization response to the client's verified `redirect_uri`.
fn client_response(
    mode: ResponseMode,
    redirect_uri: &str,
    params: &[(&str, &str)],
) -> OAuthResult<Response> {
    match mode {
        ResponseMode::Query => found_redirect(redirect_location(redirect_uri, params)?),
        ResponseMode::FormPost => form_post_response(redirect_uri, params),
    }
}

/// An HTML page whose form posts `params` to `action` as soon as it loads;
/// without JavaScript the user presses the button. Inline handlers need no
/// Content-Security-Policy exception today; adding a CSP would need one.
fn form_post_response(action: &str, params: &[(&str, &str)]) -> OAuthResult<Response> {
    let inputs = params
        .iter()
        .map(|(name, value)| {
            format!(
                r#"<input type="hidden" name="{}" value="{}">"#,
                html_escape(name),
                html_escape(value)
            )
        })
        .collect::<String>();
    let body = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><meta name="referrer" content="no-referrer"><title>Continue</title></head>
<body onload="document.forms[0].submit()">
<form method="post" action="{action}">{inputs}<noscript><button type="submit">Continue</button></noscript></form>
</body>
</html>
"#,
        action = html_escape(action)
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .header(CACHE_CONTROL, "no-store")
        .header(PRAGMA, "no-cache")
        .body(Body::from(body))
        .map_err(OAuthErrorResponse::internal)
}

fn html_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
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

    let Some(client) = crate::data::db::get_oauth_client_by_client_id(client_id)
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

pub(super) async fn current_authorized_user(
    token: Option<String>,
) -> OAuthResult<Option<AuthorizedUser>> {
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
    let Some(subject) = crate::data::store::use_store()
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
    let Some(user) = crate::data::db::get_user_by_id(user_id)
        .await
        .map_err(OAuthErrorResponse::internal)?
    else {
        return Ok(None);
    };

    let Some(auth_time) = claims.auth_time else {
        return Ok(None);
    };
    let Some(session) = crate::act::sessions::get_active_session(sid, &user.id)
        .await
        .map_err(OAuthErrorResponse::internal)?
    else {
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
        amr: session.amr,
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

    let Some(consent) = crate::data::db::get_active_oauth_consent(&user.id, &client.client_id)
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
    authentication: oidc::authentication::Authentication,
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
        authentication,
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
    fn response_modes() {
        assert_eq!(ResponseMode::parse(None), Some(ResponseMode::Query));
        assert_eq!(
            ResponseMode::parse(Some("query")),
            Some(ResponseMode::Query)
        );
        assert_eq!(
            ResponseMode::parse(Some(" form_post ")),
            Some(ResponseMode::FormPost)
        );
        assert_eq!(ResponseMode::parse(Some("fragment")), None);
        assert_eq!(ResponseMode::parse(Some("jwt")), None);
    }

    #[tokio::test]
    async fn form_post_escapes_the_action_and_values() {
        let response = form_post_response(
            "https://app.example.com/cb?a=1&b=\"2\"",
            &[("code", "c<1>"), ("state", "s'&\"")],
        )
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();

        assert!(body.contains(
            r#"<form method="post" action="https://app.example.com/cb?a=1&amp;b=&quot;2&quot;">"#
        ));
        assert!(body.contains(r#"<input type="hidden" name="code" value="c&lt;1&gt;">"#));
        assert!(body.contains(r#"<input type="hidden" name="state" value="s&#39;&amp;&quot;">"#));
    }

    #[test]
    fn max_age_zero_always_requires_login() {
        let user = AuthorizedUser {
            user: db::ent::User::new("alice@example.com".to_string(), "alice".to_string()),
            sid: "sid-1".to_string(),
            auth_time: Utc::now(),
            amr: vec!["pwd".to_string()],
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
