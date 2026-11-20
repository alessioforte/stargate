pub mod expired;
pub mod forgot;
pub mod reset;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ChangePasswordRequestBody {
    pub token: String,
    pub password: String,
}

#[cfg(test)]
mod credential_security;
