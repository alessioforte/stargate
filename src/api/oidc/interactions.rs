//! The browser side of parked authorization requests (see
//! `iam::oidc::interactions` for the state).
//!
//! When `/oauth/authorize` needs an interactive login, the browser is sent to
//! the hosted auth app's login page with
//! `return_to=/oauth/authorize?interaction={id}`, which resumes the original
//! request after sign-in. The id also travels in the [`INTERACTION_COOKIE`]
//! for the page's no-JavaScript form post to `/oauth/login` (see `login.rs`).
//! Consent and step-up send the browser to the app's consent page (see
//! `consent.rs`) and step-up page.

pub(super) use crate::iam::oidc::interactions::{
    ConsentDecision, ConsentStage, INTERACTION_TTL_SECS, PendingAuthorization, StepUpStage,
    decide_consent, get, park, park_for_consent, park_for_step_up, take,
};

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
    if crate::srv::tls::served_over_https() {
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
        &crate::web::links::auth_app_href(),
        &crate::conf::public_base_url(),
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

/// The hosted auth app's step-up page (email code) for a parked request.
pub(crate) fn step_up_page_url(id: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("interaction", id)
        .finish();
    format!(
        "{}/step-up?{query}",
        crate::web::links::auth_app_href().trim_end_matches('/')
    )
}

/// The hosted auth app's consent page for a parked request.
pub(crate) fn consent_page_url(id: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("interaction", id)
        .finish();
    format!(
        "{}/consent?{query}",
        crate::web::links::auth_app_href().trim_end_matches('/')
    )
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

    #[test]
    fn interaction_cookie_is_strict_and_scoped_to_the_login_endpoint() {
        let cookie = interaction_cookie("abc", 600);

        assert!(cookie.starts_with("oauth_interaction=abc; Path=/oauth/login;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Max-Age=600"));
    }
}
