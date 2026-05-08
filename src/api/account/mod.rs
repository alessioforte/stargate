pub mod credentials;
pub mod login;
pub mod logout;
pub mod profile;
pub mod refresh_token;

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
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserCredentials {
    pub username: String,
    pub password: String,
}

pub fn router() -> axum::Router {
    use axum::routing::{delete, get, post, put};
    axum::Router::new()
        .route("/account/login", post(login::post_login))
        .route("/account/logout", delete(logout::delete_logout))
        .route("/account/profile", get(profile::get_profile))
        .route(
            "/account/refresh-token",
            put(refresh_token::put_refresh_token),
        )
        .route(
            "/account/credentials",
            post(credentials::forgot::post_credentials).put(credentials::reset::put_credentials),
        )
}
