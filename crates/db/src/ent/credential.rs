use objectid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "credential_type", rename_all = "lowercase")]
pub enum CredentialType {
    Password,
    Oauth,
}

impl CredentialType {
    pub fn as_str(&self) -> &str {
        match self {
            CredentialType::Password => "password",
            // CredentialType::ApiKey => "api_key",
            CredentialType::Oauth => "oauth",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "password" => Some(CredentialType::Password),
            "oauth" => Some(CredentialType::Oauth),
            _ => None,
        }
    }
}

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub id: String,
    pub user_id: String,
    pub timestamp: i64,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub credential_type: String,
    pub value: String, // hashed password, provider user id, etc.
}

impl Credential {
    pub fn new(user_id: String, credential_type: CredentialType, value: String) -> Self {
        let id = ObjectId::new().unwrap().to_string();
        let timestamp = chrono::Utc::now().timestamp();
        Credential {
            id,
            user_id,
            timestamp,
            credential_type: credential_type.as_str().to_string(),
            value,
        }
    }
}
