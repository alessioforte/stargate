use chrono::{DateTime, Utc};
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An API key joined with its owner's organization binding, used to build
/// the gateway auth subject: user keys carry `user_api_keys.org_id`, service
/// account keys inherit `service_accounts.org_id`.
#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyAuth {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub api_key: ApiKey,
    pub org_id: Option<String>,
}

impl ApiKey {
    pub fn new(key_hash: String, label: String, attrs: Value) -> Self {
        let id = ulid::Ulid::generate().to_string();
        let now = Utc::now();
        ApiKey {
            id,
            key_hash,
            label,
            revoked: false,
            attrs,
            created_at: now,
            updated_at: now,
        }
    }
}
