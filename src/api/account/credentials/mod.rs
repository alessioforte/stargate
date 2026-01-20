pub mod forgot;
pub mod reset;

use actix_web::{Scope, web};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub fn routes() -> Scope {
    web::scope("/credentials")
        .service(forgot::handler)
        .service(reset::handler)
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ChangePasswordRequestBody {
    token: String,
    password: String,
}
