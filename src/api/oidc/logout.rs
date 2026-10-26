//! OpenID Connect RP-Initiated Logout 1.0: the `end_session_endpoint`.
//!
//! A relying party sends the browser to `/oauth/logout`, ideally with the
//! `id_token_hint` it received. The provider ends the browser's login session
//! (and through it the relying parties linked to it, see
//! `act::backchannel_logout`) and returns to the client's registered
//! `post_logout_redirect_uri`, or shows the hosted auth app's signed-out page.
//!
//! The session ends at once only when the hint names the signed-in user's
//! current session. Otherwise (no hint, another user, another session) the
//! request is parked and the user confirms on the hosted auth app's logout
//! page, so a cross-site link cannot sign users out. That page is
//! prerendered: its form posts to `/oauth/logout/confirm` with or without
//! JavaScript, and the parked request travels in the [`LOGOUT_COOKIE`].
//!
//! Malformed requests (invalid hint, unregistered redirect) are answered
//! with `400` and never redirect, as for `/oauth/authorize` before the
//! client is verified. `logout_hint` and `ui_locales` are accepted and
//! ignored.

use super::authorize::{AuthorizedUser, current_authorized_user};
use super::shared::{OAuthResult, oauth_bad_request};
use crate::err::OAuthErrorResponse;
use crate::web::request::RequestExt;
use axum::body::Body;
use axum::extract::{Form, FromRequest, Query, Request};
use axum::http::StatusCode;
use axum::response::Response;
use http::HeaderValue;
use http::header::{CACHE_CONTROL, LOCATION, SET_COOKIE};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use store::Store;
use url::Url;

const KEY_PREFIX: &str = "oauth:logout-request:";
const LOGOUT_REQUEST_TTL_SECS: u64 = 600;
pub(crate) const LOGOUT_ENDPOINT_PATH: &str = "/oauth/logout";

/// Carries the parked request id from the logout page's form post to
/// `/oauth/logout/confirm`. `SameSite=Strict`, so a cross-site post cannot
/// confirm a logout.
const LOGOUT_COOKIE: &str = "oauth_logout";

/// How long the browser waits for Back-Channel Logout delivery before it is
/// redirected; delivery continues afterwards.
const BACKCHANNEL_WAIT: Duration = Duration::from_secs(3);

/// RP-Initiated Logout 1.0 §2 request parameters.
#[derive(Clone, Debug, Default, Deserialize, utoipa::IntoParams)]
pub struct LogoutQuery {
    /// ID token previously issued to the client; may be expired.
    id_token_hint: Option<String>,
    client_id: Option<String>,
    /// Must exactly match one of the client's `post_logout_redirect_uris`.
    post_logout_redirect_uri: Option<String>,
    /// Returned to `post_logout_redirect_uri` unchanged.
    state: Option<String>,
}

/// A logout request waiting for the user's confirmation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct PendingLogout {
    /// Validated `post_logout_redirect_uri`, with `state`.
    redirect: Option<String>,
}

/// A validated request: where to go afterwards and which session the
/// relying party asks to end.
struct LogoutRequest {
    redirect: Option<String>,
    /// `sub` and `sid` of the `id_token_hint`.
    hinted: Option<(String, Option<String>)>,
}

#[derive(Debug, Deserialize)]
struct ConfirmForm {
    /// `logout` (default) or `cancel`.
    action: Option<String>,
}

#[utoipa::path(
    get,
    path = "/oauth/logout",
    tags = ["OAuth"],
    params(LogoutQuery),
    responses(
        (status = 303, description = "Signed out: to the post_logout_redirect_uri or the signed-out page. Or to the hosted logout page when the user must confirm."),
        (status = 400, description = "Invalid id_token_hint, client_id, or post_logout_redirect_uri", body = OAuthErrorResponse),
    )
)]
pub async fn get_logout(req: Request) -> OAuthResult<Response> {
    let token = req.get_token();
    let Query(query) = Query::<LogoutQuery>::try_from_uri(req.uri())
        .map_err(|e| oauth_bad_request(e.body_text()))?;
    end_session_request(token, query).await
}

#[utoipa::path(
    post,
    path = "/oauth/logout",
    tags = ["OAuth"],
    request_body(content = String, content_type = "application/x-www-form-urlencoded", description = "Logout request parameters (RP-Initiated Logout 1.0 §2)"),
    responses(
        (status = 303, description = "Same as GET. A cross-site post, which carries no session cookie, is redirected to the GET form first."),
        (status = 400, description = "Invalid id_token_hint, client_id, or post_logout_redirect_uri", body = OAuthErrorResponse),
    )
)]
pub async fn post_logout(req: Request) -> OAuthResult<Response> {
    let token = req.get_token();
    let Form(query) = Form::<LogoutQuery>::from_request(req, &())
        .await
        .map_err(|e| oauth_bad_request(e.body_text()))?;
    if token.is_none() {
        // The session cookie is SameSite=Lax: a relying party's cross-site
        // form post arrives without it. The redirected GET carries it.
        return see_other(&get_location(&query)?);
    }
    end_session_request(token, query).await
}

#[utoipa::path(
    post,
    path = "/oauth/logout/confirm",
    tags = ["OAuth"],
    request_body(content = String, content_type = "application/x-www-form-urlencoded", description = "action=logout (default) or action=cancel, posted by the hosted logout page; the parked request comes from the oauth_logout cookie"),
    responses(
        (status = 303, description = "To the post_logout_redirect_uri, the signed-out page, or (cancel) the auth app; expired requests return to the logout page with an error code"),
    )
)]
pub async fn post_logout_confirm(req: Request) -> OAuthResult<Response> {
    let token = req.get_token();
    let id = req.get_cookie_value(LOGOUT_COOKIE);
    let action = Form::<ConfirmForm>::from_request(req, &())
        .await
        .ok()
        .and_then(|Form(form)| form.action);

    let pending = match id {
        Some(id) => take(&id).await?,
        None => None,
    };
    let Some(pending) = pending else {
        return see_other(&logout_page_url(Some("logout_expired")));
    };

    let mut response = if action.as_deref() == Some("cancel") {
        let home = format!(
            "{}/",
            crate::web::links::auth_app_href().trim_end_matches('/')
        );
        see_other(pending.redirect.as_deref().unwrap_or(&home))?
    } else {
        if let Some(session) = current_authorized_user(token).await? {
            end_browser_session(&session).await?;
        }
        signed_out(pending.redirect)?
    };
    append_cookie(&mut response, &logout_cookie("", 0))?;
    Ok(response)
}

async fn end_session_request(token: Option<String>, query: LogoutQuery) -> OAuthResult<Response> {
    let request = validate(&query).await?;
    let Some(session) = current_authorized_user(token).await? else {
        // Nobody is signed in on this browser.
        return signed_out(request.redirect);
    };

    let hint_names_this_session = request.hinted.as_ref().is_some_and(|(sub, sid)| {
        *sub == session.user.id && sid.as_ref().is_none_or(|sid| *sid == session.sid)
    });
    if hint_names_this_session {
        end_browser_session(&session).await?;
        return signed_out(request.redirect);
    }

    let id = park(&PendingLogout {
        redirect: request.redirect,
    })
    .await?;
    let mut response = see_other(&logout_page_url(None))?;
    append_cookie(&mut response, &logout_cookie(&id, LOGOUT_REQUEST_TTL_SECS))?;
    Ok(response)
}

fn nonempty(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

async fn validate(query: &LogoutQuery) -> OAuthResult<LogoutRequest> {
    let hint = match nonempty(&query.id_token_hint) {
        Some(token) => Some(
            crate::etc::auth::jwt::jwt_config()
                .validate_id_token_hint(token)
                .map_err(|_| oauth_bad_request("id_token_hint is invalid"))?,
        ),
        None => None,
    };
    let requested_client_id = nonempty(&query.client_id);
    if let (Some(hint), Some(client_id)) = (&hint, requested_client_id)
        && hint.aud.as_deref() != Some(client_id)
    {
        return Err(oauth_bad_request(
            "client_id does not match the id_token_hint audience",
        ));
    }
    let client_id = requested_client_id.or_else(|| hint.as_ref().and_then(|h| h.aud.as_deref()));
    let redirect_uri = nonempty(&query.post_logout_redirect_uri);

    // An explicit client_id must exist; the hint's audience only matters
    // when it is needed to check the redirect.
    if requested_client_id.is_some() || redirect_uri.is_some() {
        let client_id = client_id.ok_or_else(|| {
            oauth_bad_request("post_logout_redirect_uri requires client_id or id_token_hint")
        })?;
        let client = crate::data::db::get_oauth_client_by_client_id(client_id)
            .await
            .map_err(OAuthErrorResponse::internal)?
            .filter(|client| client.enabled)
            .ok_or_else(|| oauth_bad_request("unknown client_id"))?;
        if let Some(uri) = redirect_uri
            && !oidc::logout::post_logout_redirect_registered(
                &client.post_logout_redirect_uris,
                uri,
            )
        {
            return Err(oauth_bad_request(
                "post_logout_redirect_uri is not registered for this client",
            ));
        }
    }

    Ok(LogoutRequest {
        redirect: redirect_uri
            .map(|uri| with_state(uri, nonempty(&query.state)))
            .transpose()?,
        hinted: hint.map(|hint| (hint.sub, hint.sid)),
    })
}

fn with_state(uri: &str, state: Option<&str>) -> OAuthResult<String> {
    let Some(state) = state else {
        return Ok(uri.to_string());
    };
    let mut url = Url::parse(uri)
        .map_err(|_| oauth_bad_request("post_logout_redirect_uri must be an absolute URI"))?;
    url.query_pairs_mut().append_pair("state", state);
    Ok(url.to_string())
}

/// `/oauth/logout` with the posted parameters, for the cross-site POST bounce.
fn get_location(query: &LogoutQuery) -> OAuthResult<String> {
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in [
        ("id_token_hint", &query.id_token_hint),
        ("client_id", &query.client_id),
        ("post_logout_redirect_uri", &query.post_logout_redirect_uri),
        ("state", &query.state),
    ] {
        if let Some(value) = value {
            params.append_pair(name, value);
        }
    }
    let params = params.finish();
    let base = format!(
        "{}{LOGOUT_ENDPOINT_PATH}",
        super::well_known::public_endpoint_base_url().trim_end_matches('/')
    );
    Ok(if params.is_empty() {
        base
    } else {
        format!("{base}?{params}")
    })
}

/// End this browser's login session and give its relying parties a moment
/// to receive their back-channel logout tokens.
async fn end_browser_session(session: &AuthorizedUser) -> OAuthResult<()> {
    crate::act::sessions::end_session(&session.user.id, &session.sid)
        .await
        .map_err(OAuthErrorResponse::internal)?
        .wait(BACKCHANNEL_WAIT)
        .await;
    Ok(())
}

/// The user is signed out: go to `redirect`, else to the signed-out page,
/// and drop the session cookie.
fn signed_out(redirect: Option<String>) -> OAuthResult<Response> {
    let location = redirect.unwrap_or_else(signed_out_page_url);
    let mut response = see_other(&location)?;
    append_cookie(&mut response, &crate::web::cookies::build_jwt_cookie("", 0))?;
    Ok(response)
}

fn see_other(location: &str) -> OAuthResult<Response> {
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(LOCATION, location)
        .header(CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .map_err(OAuthErrorResponse::internal)
}

fn append_cookie(response: &mut Response, cookie: &str) -> OAuthResult<()> {
    let value = HeaderValue::from_str(cookie).map_err(OAuthErrorResponse::internal)?;
    response.headers_mut().append(SET_COOKIE, value);
    Ok(())
}

fn logout_cookie(id: &str, max_age_secs: u64) -> String {
    let mut cookie = format!(
        "{LOGOUT_COOKIE}={id}; Path={LOGOUT_ENDPOINT_PATH}; HttpOnly; SameSite=Strict; Max-Age={max_age_secs}"
    );
    if crate::srv::tls::enabled().unwrap_or(false) {
        cookie.push_str("; Secure");
    }
    cookie
}

/// The hosted auth app's logout confirmation page.
fn logout_page_url(error: Option<&str>) -> String {
    let page = format!(
        "{}/logout",
        crate::web::links::auth_app_href().trim_end_matches('/')
    );
    match error {
        Some(error) => format!(
            "{page}?{}",
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair("error", error)
                .finish()
        ),
        None => page,
    }
}

fn signed_out_page_url() -> String {
    format!(
        "{}/signed-out",
        crate::web::links::auth_app_href().trim_end_matches('/')
    )
}

fn key(id: &str) -> String {
    format!("{KEY_PREFIX}{id}")
}

async fn park(pending: &PendingLogout) -> OAuthResult<String> {
    let id = format!("{}_{}", ulid::Ulid::generate(), pw::generate_api_key());
    crate::data::store::use_store()
        .set(&key(&id), pending, Some(LOGOUT_REQUEST_TTL_SECS))
        .await
        .map_err(OAuthErrorResponse::internal)?;
    Ok(id)
}

/// Load and delete a parked request so it is confirmed at most once.
async fn take(id: &str) -> OAuthResult<Option<PendingLogout>> {
    let valid_id = !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !valid_id {
        return Ok(None);
    }
    let store = crate::data::store::use_store();
    let Some(pending) = store
        .get::<PendingLogout>(&key(id))
        .await
        .map_err(OAuthErrorResponse::internal)?
    else {
        return Ok(None);
    };
    if !store
        .delete(&key(id))
        .await
        .map_err(OAuthErrorResponse::internal)?
    {
        return Ok(None);
    }
    Ok(Some(pending))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_is_appended_to_the_registered_uri() {
        assert_eq!(
            with_state("https://app.example/bye", Some("s 1")).unwrap(),
            "https://app.example/bye?state=s+1"
        );
        assert_eq!(
            with_state("https://app.example/bye?lang=it", Some("s1")).unwrap(),
            "https://app.example/bye?lang=it&state=s1"
        );
        assert_eq!(
            with_state("https://app.example/bye", None).unwrap(),
            "https://app.example/bye"
        );
    }

    #[test]
    fn logout_cookie_is_strict_and_scoped_to_the_logout_endpoint() {
        let cookie = logout_cookie("abc", 600);

        assert!(cookie.starts_with("oauth_logout=abc; Path=/oauth/logout;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Max-Age=600"));
    }

    #[cfg(feature = "memory")]
    #[tokio::test]
    async fn parked_logout_is_confirmed_once() {
        let pending = PendingLogout {
            redirect: Some("https://app.example/bye?state=s1".to_string()),
        };
        let id = park(&pending).await.unwrap();

        assert_eq!(take(&id).await.unwrap(), Some(pending));
        assert_eq!(take(&id).await.unwrap(), None);
        assert_eq!(take("../other").await.unwrap(), None);
    }
}
