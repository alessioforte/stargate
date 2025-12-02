use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    pub id: String,
    pub account_id: String,
    pub key_hash: String,
    pub label: Option<String>,
    pub revoked: bool,
    pub attrs: Value,
}

impl ApiKey {
    pub fn new(key_hash: String, account_id: String, label: Option<String>, attrs: Value) -> Self {
        let id = ulid::Ulid::new().to_string();
        ApiKey {
            id,
            account_id,
            key_hash,
            label,
            revoked: false,
            attrs,
        }
    }
}
