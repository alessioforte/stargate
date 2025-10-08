pub mod complete;
pub mod request;
pub mod verification;

use actix_web::{Scope, web};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub fn routes() -> Scope {
    web::scope("/signup")
        .service(request::handler)
        .service(verification::handler)
        .service(complete::handler)
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignupCompleteRequestBody {
    token: String,
    given_name: String,
    family_name: String,
    nickname: String,
    password: String,
    phone_number: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignupVerificationParams {
    token: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SignupRequestBody {
    email: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EmailVerificationResponse {
    email: String,
    token: String,
}
