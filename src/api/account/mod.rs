pub mod credentials;
pub mod login;
pub mod logout;
pub mod organizations;
pub mod otp;
pub mod password_policy;
pub mod profile;
pub mod refresh_token;
mod session;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RefreshTokenRequestBody {
    pub refresh_token: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    /// The organization this session acts in, when the user has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserCredentials {
    pub username: String,
    pub password: String,
    /// Optional org to act in for this session; must be one of the user's
    /// memberships. Defaults to `users.attrs.default_org`, then the oldest
    /// membership.
    #[serde(default, rename = "orgId")]
    pub org_id: Option<String>,
}

pub fn router() -> axum::Router {
    use axum::middleware::from_fn;
    use axum::routing::{delete, get, post, put};

    let sensitive = axum::Router::new()
        .route("/account/login", post(login::post_login))
        .route("/account/logout", delete(logout::delete_logout))
        .route("/account/logout/all", post(logout::post_logout_all))
        .route(
            "/account/login/otp/email",
            post(otp::post_login_email_otp).put(otp::put_login_email_otp),
        )
        .route(
            "/account/login/mfa/challenges/{challenge_id}",
            put(otp::put_login_mfa_challenge),
        )
        .route("/account/mfa/methods", get(otp::get_mfa_methods))
        .route("/account/mfa/challenges", post(otp::post_mfa_challenge))
        .route(
            "/account/mfa/challenges/{challenge_id}",
            put(otp::put_mfa_challenge),
        )
        .route(
            "/account/refresh-token",
            put(refresh_token::put_refresh_token),
        )
        .route(
            "/account/session/organization",
            put(organizations::put_session_organization),
        )
        .route(
            "/account/credentials",
            post(credentials::forgot::post_credentials).put(credentials::reset::put_credentials),
        )
        .layer(from_fn(crate::etc::origin::trusted_origin_middleware));

    axum::Router::new()
        .merge(sensitive)
        .route("/account/profile", get(profile::get_profile))
        .route(
            "/account/organizations",
            get(organizations::get_account_organizations),
        )
        .route(
            "/account/password-policy",
            get(password_policy::get_password_policy),
        )
}
