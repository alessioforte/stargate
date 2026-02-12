use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "credential_type", rename_all = "snake_case")]
pub enum CredentialType {
    Password,
    Oauth,
}

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub id: String,
    pub user_id: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub credential_type: CredentialType,
    pub value: String,
}

impl Credential {
    pub fn new(user_id: String, credential_type: CredentialType, value: String) -> Self {
        let id = ulid::Ulid::new().to_string();
        Credential {
            id,
            user_id,
            credential_type,
            value,
        }
    }
}
