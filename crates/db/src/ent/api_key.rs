use objectid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub user_id: String,
    pub key_hash: String,
    pub label: Option<String>,
    pub revoked: bool,
    pub scopes: Option<Vec<String>>,
    pub expiration: Option<i64>,
}

impl ApiKey {
    pub fn new(key_hash: String, user_id: String, label: Option<String>) -> Self {
        let id = ObjectId::new().unwrap().to_string();
        ApiKey {
            id,
            user_id,
            key_hash,
            label,
            revoked: false,
            scopes: None,
            expiration: None,
        }
    }
}
