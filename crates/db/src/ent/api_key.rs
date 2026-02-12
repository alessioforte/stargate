use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    pub id: String,
    pub key_hash: String,
    pub label: String,
    pub revoked: bool,
    pub attrs: Value,
}

impl ApiKey {
    pub fn new(key_hash: String, label: String, attrs: Value) -> Self {
        let id = ulid::Ulid::new().to_string();
        ApiKey {
            id,
            key_hash,
            label,
            revoked: false,
            attrs,
        }
    }
}
