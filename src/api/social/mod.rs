//! Sign-in with Google and GitHub.
//!
//! Browsers start at `/oauth/{provider}/start`, which binds a `state` to a
//! cookie and redirects to the provider; its callback (`/oauth/{provider}`)
//! signs in and redirects to the auth app. API clients instead take a
//! `state` from `POST /oauth/state`, run the provider's authorization
//! themselves, and get the tokens from the callback as JSON.

pub mod github;
pub mod google;
pub(crate) mod state;

pub use state::post_state;

use crate::api::account::session::{session_cookie, session_response};
use crate::conf::features::{SelfServiceRefusal, features};
use crate::ctx::audit_request_from;
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::fed::state::{BrowserSignIn, OAuthState, create_oauth_state, take_oauth_state};
use crate::iam::sess::issuance::{IssuedSession, start_user_session};
use crate::web::request::RequestExt;
use axum::extract::{Query, Request};
use axum::response::{IntoResponse, Response};
use db::ent::TrustedAuditRequest;
use http::header::{CACHE_CONTROL, LOCATION, SET_COOKIE};
use http::{HeaderValue, StatusCode};
use serde::Deserialize;

/// Binds a browser sign-in to the browser that started it, so a callback URL
/// from someone else's sign-in cannot sign this browser in (login CSRF).
const STATE_COOKIE: &str = "social_state";
/// As long as the state itself (`iam::fed::state`).
const STATE_COOKIE_MAX_AGE_SECS: u64 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Provider {
    Google,
    GitHub,
}

impl Provider {
    /// The name in error params and logs.
    const fn display_name(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::GitHub => "GitHub",
        }
    }

    /// The name in URLs and in `/account/features`.
    const fn slug(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::GitHub => "github",
        }
    }

    fn authorize_url(
        self,
        state: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        match self {
            Self::Google => idp::google::google_authorize_url(state),
            Self::GitHub => idp::github::github_authorize_url(state),
        }
    }

    async fn sign_in(
        self,
        code: &str,
        audit_request: TrustedAuditRequest,
    ) -> Result<db::ent::User, ErrorResponse> {
        match self {
            Self::Google => google::sign_in(code, audit_request).await,
            Self::GitHub => github::sign_in(code, audit_request).await,
        }
    }
}

/// A first sign-in creates the account only as self-registration would
/// (`SIGNUP_MODE=open`, an allowed email domain); otherwise the person needs
/// an invitation or an account created by an administrator.
fn ensure_provisioning(provider: &'static str, email: &str) -> Result<(), ErrorResponse> {
    match features().self_service_signup(email) {
        Ok(()) => Ok(()),
        Err(SelfServiceRefusal::Disabled) => {
            Err(ErrorResponse::new(ErrorCode::SocialAccountNotProvisioned)
                .with_param("provider", provider))
        }
        Err(SelfServiceRefusal::DomainNotAllowed(domain)) => {
            Err(ErrorResponse::new(ErrorCode::SignupDomainNotAllowed).with_param("domain", domain))
        }
    }
}

#[derive(Debug, Deserialize)]
struct StartQuery {
    return_to: Option<String>,
}

/// The origin the browser reached this server at: its `Host`, with `https`
/// when this server or the proxy in front of it terminates TLS
/// ([`crate::srv::tls::served_over_https`]).
fn request_origin(req: &Request) -> Option<String> {
    let host = req.headers().get(http::header::HOST)?.to_str().ok()?;
    let scheme = if crate::srv::tls::served_over_https() {
        "https"
    } else {
        "http"
    };
    Some(format!("{scheme}://{host}"))
}

/// `/oauth/{provider}/start`: a state bound to this browser, then the
/// provider's authorization page. `own_origin` is [`request_origin`].
async fn start(
    provider: Provider,
    uri: http::Uri,
    own_origin: Option<String>,
) -> Result<Response, ErrorResponse> {
    let Query(query) = Query::<StartQuery>::try_from_uri(&uri).map_err(|error| {
        ErrorResponse::new(ErrorCode::RequestInvalidQuery).with_message(error.body_text())
    })?;
    let return_to = match query
        .return_to
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => None,
        Some(value) => Some(safe_return_to(value, own_origin).ok_or_else(|| {
            tracing::warn!(
                return_to = value,
                "social sign-in refused a return_to outside this deployment (allowed: paths, and the origins of JWT_ISSUER, OAUTH_BASE_URL, AUTH_PUBLIC_BASE_URL, TRUSTED_ORIGINS, CORS_ORIGINS, and this server)"
            );
            ErrorResponse::new(ErrorCode::RequestInvalidQuery).with_param("field", "return_to")
        })?),
    };

    let record = OAuthState {
        browser: Some(BrowserSignIn {
            provider: provider.slug().to_string(),
            return_to,
        }),
    };
    let state = create_oauth_state(&record).await.map_err(|error| {
        tracing::error!(%error, "failed to create OAuth state");
        ErrorResponse::new(ErrorCode::SocialStateGenerationFailed)
    })?;
    let location = provider.authorize_url(&state).map_err(|error| {
        tracing::error!(%error, provider = provider.display_name(), "cannot build the authorization URL");
        ErrorResponse::new(ErrorCode::SystemInternal)
    })?;
    Ok(redirect(
        &location,
        &[state_cookie(&state, STATE_COOKIE_MAX_AGE_SECS)],
    ))
}

#[derive(Debug, Default, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    /// Set by the provider when the user did not authorize.
    error: Option<String>,
}

/// `/oauth/{provider}`, where the provider returns the browser.
async fn callback(req: Request, provider: Provider) -> Response {
    let audit_request = audit_request_from(req.extensions());
    let bound_state = req.get_cookie_value(STATE_COOKIE);
    let query = match Query::<CallbackQuery>::try_from_uri(req.uri()) {
        Ok(Query(query)) => query,
        Err(error) if bound_state.is_none() => {
            return ErrorResponse::new(ErrorCode::RequestInvalidQuery)
                .with_message(error.body_text())
                .into_response();
        }
        Err(_) => CallbackQuery::default(),
    };
    let record = match nonempty(query.state.as_deref()) {
        Some(state) => take_oauth_state(state).await.unwrap_or_else(|error| {
            tracing::error!(%error, "failed to read OAuth state");
            None
        }),
        None => None,
    };

    match record {
        Some(OAuthState {
            browser: Some(browser),
        }) => browser_callback(provider, browser, query, bound_state, audit_request).await,
        // The browser started a sign-in whose state expired or was used.
        None if bound_state.is_some() => login_failure(None, LoginError::Failed),
        Some(OAuthState { browser: None }) => {
            match api_callback(provider, &query, audit_request).await {
                Ok(response) => response,
                Err(error) => error.into_response(),
            }
        }
        None => {
            let code = if nonempty(query.code.as_deref()).is_none() {
                ErrorCode::SocialAuthorizationCodeMissing
            } else {
                ErrorCode::SocialStateInvalid
            };
            ErrorResponse::new(code).into_response()
        }
    }
}

async fn browser_callback(
    provider: Provider,
    browser: BrowserSignIn,
    query: CallbackQuery,
    bound_state: Option<String>,
    audit_request: TrustedAuditRequest,
) -> Response {
    let return_to = browser.return_to.as_deref();
    // Only the browser that started this sign-in may finish it.
    if bound_state.as_deref() != query.state.as_deref() || browser.provider != provider.slug() {
        tracing::warn!(
            provider = provider.display_name(),
            "social sign-in state does not match this browser"
        );
        return login_failure(return_to, LoginError::Failed);
    }
    if query.error.is_some() {
        return login_failure(return_to, LoginError::Cancelled);
    }
    let Some(code) = nonempty(query.code.as_deref()) else {
        return login_failure(return_to, LoginError::Failed);
    };
    match signed_in(provider, code, audit_request).await {
        Ok(session) => {
            let params: Vec<(&str, &str)> = return_to
                .map(|return_to| vec![("return_to", return_to)])
                .unwrap_or_default();
            // Through the organization picker, as after a password sign-in.
            let location = crate::web::links::auth_app_redirect("select-organization", &params);
            redirect(&location, &[session_cookie(&session), state_cookie("", 0)])
        }
        Err(error) => {
            tracing::debug!(code = ?error.code, provider = provider.display_name(), "social sign-in refused");
            login_failure(return_to, LoginError::from(&error))
        }
    }
}

async fn api_callback(
    provider: Provider,
    query: &CallbackQuery,
    audit_request: TrustedAuditRequest,
) -> Result<Response, ErrorResponse> {
    let code = nonempty(query.code.as_deref())
        .ok_or_else(|| ErrorResponse::new(ErrorCode::SocialAuthorizationCodeMissing))?;
    session_response(signed_in(provider, code, audit_request).await?)
}

async fn signed_in(
    provider: Provider,
    code: &str,
    audit_request: TrustedAuditRequest,
) -> Result<IssuedSession, ErrorResponse> {
    let user = provider.sign_in(code, audit_request).await?;
    let auth_time = chrono::Utc::now().timestamp() as usize;
    start_user_session(
        user,
        auth_time,
        None,
        vec![oidc::authentication::AMR_FEDERATED.to_string()],
    )
    .await
}

/// Why a browser sign-in failed, as the auth app's login page reads it from
/// `?error=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoginError {
    Cancelled,
    EmailUnverified,
    AccountNotProvisioned,
    DomainNotAllowed,
    Failed,
}

impl LoginError {
    const fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "social_cancelled",
            Self::EmailUnverified => "social_email_unverified",
            Self::AccountNotProvisioned => "social_account_not_provisioned",
            Self::DomainNotAllowed => "social_domain_not_allowed",
            Self::Failed => "social_failed",
        }
    }
}

impl From<&ErrorResponse> for LoginError {
    fn from(error: &ErrorResponse) -> Self {
        match error.code {
            ErrorCode::SocialEmailUnverified => Self::EmailUnverified,
            ErrorCode::SocialAccountNotProvisioned => Self::AccountNotProvisioned,
            ErrorCode::SignupDomainNotAllowed => Self::DomainNotAllowed,
            _ => Self::Failed,
        }
    }
}

/// Back to the auth app's login page, which shows `error` and keeps
/// `return_to` for another attempt.
fn login_failure(return_to: Option<&str>, error: LoginError) -> Response {
    let mut params = vec![("error", error.code())];
    params.extend(return_to.map(|return_to| ("return_to", return_to)));
    let location = crate::web::links::auth_app_redirect("login", &params);
    redirect(&location, &[state_cookie("", 0)])
}

fn redirect(location: &str, cookies: &[String]) -> Response {
    let Ok(location) = HeaderValue::from_str(location) else {
        return ErrorResponse::internal("invalid redirect location").into_response();
    };
    let mut response = StatusCode::FOUND.into_response();
    let headers = response.headers_mut();
    headers.insert(LOCATION, location);
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    for cookie in cookies {
        if let Ok(value) = HeaderValue::from_str(cookie) {
            headers.append(SET_COOKIE, value);
        }
    }
    response
}

/// `SameSite=Lax`, so it comes back with the provider's top-level redirect
/// to the callback. An empty value with `max_age` 0 clears it.
fn state_cookie(value: &str, max_age_secs: u64) -> String {
    let secure = if crate::srv::tls::served_over_https() {
        "; Secure"
    } else {
        ""
    };
    format!(
        "{STATE_COOKIE}={value}; Path=/oauth; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}"
    )
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// `return_to` is where the browser ends up after signing in, so it must stay
/// on this deployment: a path on this server, or an absolute URL on an origin
/// the deployment already trusts with browsers (the issuer, the public
/// endpoints, the auth app, `TRUSTED_ORIGINS`, `CORS_ORIGINS`) or on the one
/// this request reached.
fn safe_return_to(value: &str, own_origin: Option<String>) -> Option<String> {
    let mut bases = vec![
        jwt::issuer_from_env(),
        crate::conf::public_base_url(),
        std::env::var("AUTH_PUBLIC_BASE_URL").unwrap_or_default(),
    ];
    for name in ["TRUSTED_ORIGINS", "CORS_ORIGINS"] {
        bases.extend(origin_list(&std::env::var(name).unwrap_or_default()));
    }
    bases.extend(own_origin);
    return_to_within(value, &bases)
}

/// The entries of a comma-separated origin list, without the `*` wildcard.
fn origin_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty() && *origin != "*")
        .map(str::to_string)
        .collect()
}

fn return_to_within(value: &str, bases: &[String]) -> Option<String> {
    if !value.is_ascii() || value.chars().any(|c| c.is_ascii_control()) {
        return None;
    }
    if value.starts_with('/') {
        let other_host = value.starts_with("//") || value.starts_with("/\\");
        return (!other_host).then(|| value.to_string());
    }
    let url = url::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let origin = url.origin().ascii_serialization();
    bases
        .iter()
        .filter_map(|base| url::Url::parse(base).ok())
        .any(|base| base.origin().ascii_serialization() == origin)
        .then(|| url.to_string())
}

/// Each route answers `404 feature.disabled` while its provider has no
/// credentials (`/oauth/state` while none has).
pub fn router() -> axum::Router {
    use crate::api::middleware::features::require_feature;
    use crate::conf::features::Feature;
    use axum::middleware::{from_fn, from_fn_with_state};
    use axum::routing::{get, post};

    let state = axum::Router::new()
        .route("/oauth/state", post(post_state))
        .layer(from_fn(crate::web::origin::trusted_origin_middleware))
        .layer(from_fn_with_state(Feature::Social, require_feature));
    let github = axum::Router::new()
        .route("/oauth/github", get(github::get_github))
        .route("/oauth/github/start", get(github::get_github_start))
        .layer(from_fn_with_state(Feature::SocialGithub, require_feature));
    let google = axum::Router::new()
        .route("/oauth/google", get(google::get_google))
        .route("/oauth/google/start", get(google::get_google_start))
        .layer(from_fn_with_state(Feature::SocialGoogle, require_feature));

    axum::Router::new().merge(state).merge(github).merge(google)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bases() -> Vec<String> {
        vec![
            "https://id.example.com".to_string(),
            "https://login.example.com/auth".to_string(),
        ]
    }

    #[test]
    fn return_to_stays_on_this_deployment() {
        for allowed in [
            "/auth/consent?interaction=1",
            "https://id.example.com/oauth/authorize?interaction=01J",
            "https://login.example.com/somewhere",
        ] {
            assert!(return_to_within(allowed, &bases()).is_some(), "{allowed}");
        }
        for refused in [
            "//evil.example.com/x",
            "/\\evil.example.com",
            "https://evil.example.com/oauth/authorize",
            "https://id.example.com.evil.example/",
            "http://id.example.com/plain-http",
            "javascript:alert(1)",
            "relative/path",
            "/caf\u{e9}",
            "/line\nbreak",
        ] {
            assert_eq!(return_to_within(refused, &bases()), None, "{refused}");
        }
    }

    #[test]
    fn origin_lists_skip_blanks_and_the_wildcard() {
        assert_eq!(
            origin_list(" https://app.example.com, ,*,http://localhost:5173 "),
            ["https://app.example.com", "http://localhost:5173"]
        );
        let mut bases = bases();
        bases.extend(origin_list("http://localhost:5173"));
        assert!(return_to_within("http://localhost:5173/auth/x", &bases).is_some());
    }

    #[test]
    fn refusals_map_to_login_page_errors() {
        let cases = [
            (ErrorCode::SocialEmailUnverified, "social_email_unverified"),
            (
                ErrorCode::SocialAccountNotProvisioned,
                "social_account_not_provisioned",
            ),
            (
                ErrorCode::SignupDomainNotAllowed,
                "social_domain_not_allowed",
            ),
            (ErrorCode::SocialTokenExchangeFailed, "social_failed"),
        ];
        for (code, expected) in cases {
            assert_eq!(LoginError::from(&ErrorResponse::new(code)).code(), expected);
        }
    }

    #[test]
    fn the_state_cookie_rides_top_level_redirects_only_under_oauth() {
        let cookie = state_cookie("01J", 300);
        assert!(
            cookie
                .starts_with("social_state=01J; Path=/oauth; HttpOnly; SameSite=Lax; Max-Age=300")
        );
        assert!(state_cookie("", 0).contains("Max-Age=0"));
    }
}
