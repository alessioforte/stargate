pub mod complete;
pub mod confirm;
pub mod request;

use actix_web::{web, Scope};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub fn routes() -> Scope {
    web::scope("/signup")
        .service(request::handler)
        .service(confirm::handler)
        .service(complete::handler)
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SignupCompleteRequestBody {
    token: String,
    name: String,
    nickname: String,
    password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignupConfirmParams {
    token: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SignupRequestBody {
    email: String,
}
