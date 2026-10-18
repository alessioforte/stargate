//! Authorization requests parked while the user signs in or consents.
//!
//! When `/oauth/authorize` needs an interactive login, the request is stored
//! under an unguessable id and the browser is sent to the hosted auth app's
//! login page with `return_to=/oauth/authorize?interaction={id}`, which
//! resumes the original request after sign-in. The id also travels in the
//! [`INTERACTION_COOKIE`] for the page's no-JavaScript form post to
//! `/oauth/login` (see `login.rs`).
//!
//! When the signed-in user has not consented to the client, the request is
//! parked again with a [`ConsentStage`] bound to that user, and the browser
//! goes to the app's consent page (see `consent.rs`).

use super::authorize::AuthorizeQuery;
use super::shared::OAuthResult;
use crate::err::OAuthErrorResponse;
use serde::{Deserialize, Serialize};
use store::Store;

const KEY_PREFIX: &str = "oauth:authorize-interaction:";
pub(crate) const INTERACTION_TTL_SECS: u64 = 600;

/// Endpoint the login page's form posts to when JavaScript is unavailable.
pub(crate) const LOGIN_ENDPOINT_PATH: &str = "/oauth/login";

/// Carries the parked request id to [`LOGIN_ENDPOINT_PATH`]. Being
/// `SameSite=Strict`, it is never sent with a cross-site post, so it also
/// guards the form post against login CSRF.
pub(crate) const INTERACTION_COOKIE: &str = "oauth_interaction";

pub(crate) fn interaction_cookie(id: &str, max_age_secs: u64) -> String {
    let mut cookie = format!(
        "{INTERACTION_COOKIE}={id}; Path={LOGIN_ENDPOINT_PATH}; HttpOnly; SameSite=Strict; Max-Age={max_age_secs}"
    );
    if crate::etc::server::tls::enabled().unwrap_or(false) {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Path that resumes a parked request.
pub(crate) fn resume_path(id: &str) -> String {
    format!("/oauth/authorize?interaction={id}")
}

/// Query of the hosted auth app's login page.
#[derive(Default)]
pub(crate) struct LoginPage<'a> {
    /// Parked request to resume after sign-in.
    pub(crate) interaction: Option<&'a str>,
    /// Error code the page displays (no-JavaScript form post).
    pub(crate) error: Option<&'a str>,
    /// The relying party's `login_hint`, prefilled as the username.
    pub(crate) login_hint: Option<&'a str>,
}

/// The hosted auth app's login page URL. `return_to` is absolute because
/// the app treats relative values as its own routes.
pub(crate) fn login_page_url(page: LoginPage) -> String {
    login_page_url_from(
        &crate::fun::auth_app_href(),
        &super::well_known::public_endpoint_base_url(),
        page,
    )
}

fn login_page_url_from(auth_app: &str, endpoint_base: &str, page: LoginPage) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    if let Some(id) = page.interaction {
        let resume_url = format!("{}{}", endpoint_base.trim_end_matches('/'), resume_path(id));
        query.append_pair("return_to", &resume_url);
    }
    if let Some(error) = page.error {
        query.append_pair("error", error);
    }
    if let Some(hint) = page
        .login_hint
        .map(str::trim)
        .filter(|hint| !hint.is_empty())
    {
        query.append_pair("login_hint", hint);
    }
    let query = query.finish();

    let login = format!("{}/login", auth_app.trim_end_matches('/'));
    if query.is_empty() {
        login
    } else {
        format!("{login}?{query}")
    }
}

/// The hosted auth app's consent page for a parked request.
pub(crate) fn consent_page_url(id: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("interaction", id)
        .finish();
    format!(
        "{}/consent?{query}",
        crate::fun::auth_app_href().trim_end_matches('/')
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PendingAuthorization {
    pub(crate) query: AuthorizeQuery,
    /// Unix seconds when the request was parked. A session whose `auth_time`
    /// is not older than this was authenticated for this request.
    pub(crate) created_at: i64,
    /// Present when the request waits for the user's consent.
    #[serde(default)]
    pub(crate) consent: Option<ConsentStage>,
}

/// A request parked for consent. The login checks already passed for
/// `user_id`, so resuming only needs that same user's session.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct ConsentStage {
    pub(crate) user_id: String,
    /// Scopes and audience the client asked for, after validation.
    pub(crate) scopes: Vec<String>,
    pub(crate) audience: Option<String>,
    /// Set once the user approves on the consent page.
    #[serde(default)]
    pub(crate) approved: bool,
}

fn key(id: &str) -> String {
    format!("{KEY_PREFIX}{id}")
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub(crate) async fn park(query: AuthorizeQuery) -> OAuthResult<String> {
    park_with(query, None).await
}

pub(crate) async fn park_for_consent(
    query: AuthorizeQuery,
    stage: ConsentStage,
) -> OAuthResult<String> {
    park_with(query, Some(stage)).await
}

async fn park_with(query: AuthorizeQuery, consent: Option<ConsentStage>) -> OAuthResult<String> {
    let id = format!("{}_{}", ulid::Ulid::generate(), pw::generate_api_key());
    let pending = PendingAuthorization {
        query,
        created_at: chrono::Utc::now().timestamp(),
        consent,
    };
    save(&id, &pending).await?;
    Ok(id)
}

async fn save(id: &str, pending: &PendingAuthorization) -> OAuthResult<()> {
    crate::etc::store::use_store()
        .set(&key(id), pending, Some(INTERACTION_TTL_SECS))
        .await
        .map_err(OAuthErrorResponse::internal)
}

/// Mark a request parked for consent as approved, so resuming it issues a
/// code even with `prompt=consent`. Returns false when there is no such
/// request for `user_id`.
pub(crate) async fn approve_consent(id: &str, user_id: &str) -> OAuthResult<bool> {
    let Some(mut pending) = get(id).await? else {
        return Ok(false);
    };
    let Some(stage) = pending
        .consent
        .as_mut()
        .filter(|stage| stage.user_id == user_id)
    else {
        return Ok(false);
    };
    stage.approved = true;
    save(id, &pending).await?;
    Ok(true)
}

pub(crate) async fn get(id: &str) -> OAuthResult<Option<PendingAuthorization>> {
    if !valid_id(id) {
        return Ok(None);
    }
    crate::etc::store::use_store()
        .get(&key(id))
        .await
        .map_err(OAuthErrorResponse::internal)
}

/// Load and delete a parked request so it resumes at most once.
pub(crate) async fn take(id: &str) -> OAuthResult<Option<PendingAuthorization>> {
    let Some(pending) = get(id).await? else {
        return Ok(None);
    };
    let deleted = crate::etc::store::use_store()
        .delete(&key(id))
        .await
        .map_err(OAuthErrorResponse::internal)?;
    if !deleted {
        return Ok(None);
    }
    Ok(Some(pending))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_page_url_returns_to_the_parked_request() {
        assert_eq!(
            login_page_url_from(
                "/auth",
                "https://id.example.com/",
                LoginPage {
                    interaction: Some("abc"),
                    ..LoginPage::default()
                }
            ),
            "/auth/login?return_to=https%3A%2F%2Fid.example.com%2Foauth%2Fauthorize%3Finteraction%3Dabc"
        );
        assert_eq!(
            login_page_url_from(
                "https://login.example.com/",
                "https://id.example.com",
                LoginPage {
                    interaction: Some("abc"),
                    error: Some("invalid_credentials"),
                    login_hint: Some(" alice@example.com "),
                }
            ),
            "https://login.example.com/login?return_to=https%3A%2F%2Fid.example.com%2Foauth%2Fauthorize%3Finteraction%3Dabc&error=invalid_credentials&login_hint=alice%40example.com"
        );
        assert_eq!(
            login_page_url_from(
                "/auth",
                "https://id.example.com",
                LoginPage {
                    error: Some("interaction_expired"),
                    login_hint: Some("  "),
                    ..LoginPage::default()
                }
            ),
            "/auth/login?error=interaction_expired"
        );
    }

    #[cfg(feature = "memory")]
    #[tokio::test]
    async fn consent_approval_is_bound_to_the_parked_user() {
        let stage = ConsentStage {
            user_id: "user-1".to_string(),
            scopes: vec!["openid".to_string()],
            audience: None,
            approved: false,
        };
        let id = park_for_consent(AuthorizeQuery::default(), stage)
            .await
            .unwrap();

        assert!(!approve_consent(&id, "user-2").await.unwrap());
        assert!(!get(&id).await.unwrap().unwrap().consent.unwrap().approved);

        assert!(approve_consent(&id, "user-1").await.unwrap());
        assert!(get(&id).await.unwrap().unwrap().consent.unwrap().approved);
        assert!(!approve_consent("missing", "user-1").await.unwrap());
    }

    #[test]
    fn interaction_cookie_is_strict_and_scoped_to_the_login_endpoint() {
        let cookie = interaction_cookie("abc", 600);

        assert!(cookie.starts_with("oauth_interaction=abc; Path=/oauth/login;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Max-Age=600"));
    }
}
