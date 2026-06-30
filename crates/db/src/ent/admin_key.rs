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
    pub revoked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AdminKey {
    pub fn new(key_hash: String, label: Option<String>, permissions: Vec<String>) -> Self {
        let id = ulid::Ulid::new().to_string();
        let now = Utc::now();
        AdminKey {
            id,
            key_hash,
            label,
            permissions: sqlx::types::Json(permissions),
            revoked: false,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn permissions_list(&self) -> &[String] {
        &self.permissions
    }

    pub fn set_permissions(&mut self, permissions: Vec<String>) {
        self.permissions = sqlx::types::Json(permissions);
    }
}
