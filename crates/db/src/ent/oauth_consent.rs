use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthConsent {
    pub id: String,
    pub client_id: String,
    pub user_id: String,
    pub scopes: sqlx::types::Json<Vec<String>>,
    pub audiences: sqlx::types::Json<Vec<String>>,
    pub granted_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub attrs: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OAuthConsent {
    pub fn new(
        client_id: String,
        user_id: String,
        scopes: Vec<String>,
        audiences: Vec<String>,
        expires_at: Option<DateTime<Utc>>,
        attrs: Value,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: ulid::Ulid::new().to_string(),
            client_id,
            user_id,
            scopes: sqlx::types::Json(scopes),
            audiences: sqlx::types::Json(audiences),
            granted_at: now,
            expires_at,
            revoked_at: None,
            attrs,
            created_at: now,
            updated_at: now,
        }
    }
}
