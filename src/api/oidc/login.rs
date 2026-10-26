//! No-JavaScript fallback of the provider login.
//!
//! `/oauth/authorize` sends browsers that need to sign in to the hosted auth
//! app's login page. That page is prerendered (`apps/auth/scripts/prerender.mjs`):
//! with JavaScript it signs in through `/account/login`, including MFA and
//! the other hosted flows, then follows `return_to` back to
//! `/oauth/authorize?interaction={id}`. Without JavaScript its form posts
//! here. This endpoint runs the password checks of `POST /account/login`
//! (login guard throttling, constant-time unknown users, rehash) and
//! answers only with redirects: back to `/oauth/authorize` on success, or to
//! the login page with an `error` code it displays.
//!
//! Accounts that need MFA or a password change are sent back with
//! `javascript_required`, since those steps live in the hosted app.

use super::interactions::{self, INTERACTION_COOKIE, LoginPage};
use crate::api::account::login::verify_login_password;
use crate::api::account::session::issue_user_session;
use crate::err::ErrorResponse;
use crate::iam::otp::service::login_mfa_required;
use crate::web::request::RequestExt;
use axum::body::Body;
use axum::extract::{Form, FromRequest, Request};
use axum::http::StatusCode;
use axum::response::Response;
use http::HeaderValue;
use http::header::{CACHE_CONTROL, LOCATION, SET_COOKIE};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct LoginForm {
    username: Option<String>,
    password: Option<String>,
}

#[utoipa::path(
    post,
    path = "/oauth/login",
    tags = ["OAuth"],
    request_body(content = String, content_type = "application/x-www-form-urlencoded", description = "username and password, posted by the hosted login page when JavaScript is unavailable; the parked authorization request comes from the oauth_interaction cookie"),
    responses(
        (status = 303, description = "Signed in: resumes the authorization request. Otherwise back to the login page with an error code."),
    )
)]
pub async fn post_login(req: Request) -> Response {
    let client_ip = req.get_client_ip();
    let interaction = req.get_cookie_value(INTERACTION_COOKIE);
    let form = Form::<LoginForm>::from_request(req, &())
        .await
        .ok()
        .map(|Form(form)| form);

    let expired = || back_to_login(LoginPage::default(), "interaction_expired");
    let Some(id) = interaction else {
        return expired();
    };
    let pending = match interactions::get(&id).await {
        Ok(Some(pending)) => pending,
        Ok(None) => return expired(),
        Err(_) => return back_to_login(LoginPage::default(), "server_error"),
    };
    let page = || LoginPage {
        interaction: Some(&id),
        login_hint: pending.query.login_hint.as_deref(),
        ..LoginPage::default()
    };

    let form = form.unwrap_or(LoginForm {
        username: None,
        password: None,
    });
    let username = form.username.as_deref().unwrap_or_default();
    let password = form.password.as_deref().unwrap_or_default();
    let user = match verify_login_password(&client_ip, username, password).await {
        Ok((user, _)) => user,
        Err(error) => return back_to_login(page(), login_error_code(&error)),
    };

    match needs_hosted_step(&user).await {
        Ok(false) => {}
        Ok(true) => return back_to_login(page(), "javascript_required"),
        Err(_) => return back_to_login(page(), "server_error"),
    }

    let auth_time = chrono::Utc::now().timestamp() as usize;
    let password = vec![oidc::authentication::AMR_PASSWORD.to_string()];
    let session = match issue_user_session(user, auth_time, None, password).await {
        Ok(session) => session,
        Err(_) => return back_to_login(page(), "server_error"),
    };

    let mut response = see_other(&interactions::resume_path(&id));
    for cookie in session.headers().get_all(SET_COOKIE) {
        response.headers_mut().append(SET_COOKIE, cookie.clone());
    }
    if let Ok(cleared) = HeaderValue::from_str(&interactions::interaction_cookie("", 0)) {
        response.headers_mut().append(SET_COOKIE, cleared);
    }
    response
}

/// MFA and password changes are only available with JavaScript.
async fn needs_hosted_step(user: &db::ent::User) -> Result<bool, ErrorResponse> {
    if login_mfa_required(user).await? {
        return Ok(true);
    }
    let Some(credential) =
        crate::data::db::get_credential(&user.id, db::ent::CredentialType::Password)
            .await
            .map_err(ErrorResponse::internal)?
    else {
        return Ok(false);
    };
    Ok(crate::iam::pwd::policy::password_expired(&user.id, credential.updated_at).await)
}

fn login_error_code(error: &ErrorResponse) -> &'static str {
    if error.status == StatusCode::TOO_MANY_REQUESTS {
        "too_many_attempts"
    } else if error.status.is_server_error() {
        "server_error"
    } else {
        "invalid_credentials"
    }
}

fn back_to_login(page: LoginPage, error: &str) -> Response {
    see_other(&interactions::login_page_url(LoginPage {
        error: Some(error),
        ..page
    }))
}

fn see_other(location: &str) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    if let Ok(location) = HeaderValue::from_str(location) {
        response.headers_mut().insert(LOCATION, location);
    }
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::err::ErrorCode;

    #[test]
    fn login_errors_map_to_page_codes() {
        let invalid = ErrorResponse::new(ErrorCode::AuthInvalidCredentials);
        let mut throttled = ErrorResponse::new(ErrorCode::AuthInvalidCredentials);
        throttled.status = StatusCode::TOO_MANY_REQUESTS;

        assert_eq!(login_error_code(&invalid), "invalid_credentials");
        assert_eq!(login_error_code(&throttled), "too_many_attempts");
        assert_eq!(
            login_error_code(&ErrorResponse::internal("boom")),
            "server_error"
        );
    }

    #[test]
    fn redirects_are_see_other_and_not_cached() {
        let response = see_other("/oauth/authorize?interaction=abc");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get(LOCATION).unwrap(),
            "/oauth/authorize?interaction=abc"
        );
        assert_eq!(response.headers().get(CACHE_CONTROL).unwrap(), "no-store");
    }
}
