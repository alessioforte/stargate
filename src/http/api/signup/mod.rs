pub mod complete;
pub mod request;
pub mod verification;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignupCompleteRequestBody {
    pub token: String,
    pub given_name: String,
    pub family_name: String,
    pub nickname: String,
    pub password: String,
    pub phone_number: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignupVerificationParams {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SignupRequestBody {
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EmailVerificationResponse {
    pub email: String,
    pub token: String,
}

pub fn router() -> axum::Router {
    use axum::routing::get;
    axum::Router::new().route(
        "/signup",
        get(verification::get_signup)
            .post(request::post_signup)
            .put(complete::put_signup),
    )
}
