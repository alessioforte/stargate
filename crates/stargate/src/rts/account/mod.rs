pub mod credentials;
pub mod login;
pub mod logout;
pub mod profile;
pub mod refresh_token;

use actix_web::{web, Scope};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub fn routes() -> Scope {
    web::scope("/account")
        .service(credentials::routes())
        .service(login::handler)
        .service(logout::handler)
        .service(profile::handler)
        .service(refresh_token::handler)
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RefreshTokenRequestBody {
    refresh_token: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ChangePasswordRequestBody {
    token: String,
    password: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    access_token: String,
    refresh_token: String,
    // expires_in: i64,
    // refresh_expires_in: i64,
    token_type: String,
    // scope: String,
    // session_state: String,
}
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserCredentials {
    username: String,
    password: String,
}
