use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminKey {
    pub id: String,
    #[serde(skip_serializing)]
    pub key_hash: String,
    pub label: Option<String>,
    pub permissions: sqlx::types::Json<Vec<String>>,
    pub allowed_listeners: Option<sqlx::types::Json<Vec<String>>>,
    pub revoked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AdminKey {
    pub fn new(
        key_hash: String,
        label: Option<String>,
        permissions: Vec<String>,
        allowed_listeners: Option<Vec<String>>,
    ) -> Self {
        let id = ulid::Ulid::generate().to_string();
        let now = Utc::now();
        AdminKey {
            id,
            key_hash,
            label,
            permissions: sqlx::types::Json(permissions),
            allowed_listeners: allowed_listeners.map(sqlx::types::Json),
            revoked: false,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn set_allowed_listeners(&mut self, names: Option<Vec<String>>) {
        self.allowed_listeners = names.map(sqlx::types::Json);
    }

    pub fn permissions_list(&self) -> &[String] {
        &self.permissions
    }

    pub fn set_permissions(&mut self, permissions: Vec<String>) {
        self.permissions = sqlx::types::Json(permissions);
    }
}
