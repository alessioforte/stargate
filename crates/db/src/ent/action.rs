use std::str::FromStr;

use objectid::ObjectId;
use serde::{Deserialize, Serialize};

pub enum ActionType {
    PasswordReset,
    EmailVerification,
    AccountDeletion,
    Signup,
}

impl FromStr for ActionType {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "password_reset" => Ok(ActionType::PasswordReset),
            "email_verification" => Ok(ActionType::EmailVerification),
            "account_deletion" => Ok(ActionType::AccountDeletion),
            "signup" => Ok(ActionType::Signup),
            _ => Err("Unknown action type"),
        }
    }
}

impl ActionType {
    pub fn as_str(&self) -> &str {
        match self {
            ActionType::PasswordReset => "password_reset",
            ActionType::EmailVerification => "email_verification",
            ActionType::AccountDeletion => "account_deletion",
            ActionType::Signup => "signup",
        }
    }
}

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub action_type: String, // e.g., "password_reset", "email_verification", "account_deletion", "signup"
    pub sub: String,
    pub value: String,
    pub iat: i64,
    pub exp: i64,
}

impl Action {
    pub fn new(
        sub: String,
        action_type: ActionType,
        duration: i64, // duration in seconds
        value: String,
    ) -> Self {
        let id = ObjectId::new().unwrap().to_string();
        let issued_at = chrono::Utc::now().timestamp();
        let expires_at = issued_at + duration;
        Action {
            id,
            action_type: action_type.as_str().to_string(),
            sub,
            value,
            iat: issued_at,
            exp: expires_at,
        }
    }
}
