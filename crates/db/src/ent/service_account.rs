use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccount {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub org_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ServiceAccount {
    pub fn new(name: String, description: Option<String>, org_id: Option<String>) -> Self {
        let id = ulid::Ulid::generate().to_string();
        let now = Utc::now();
        ServiceAccount {
            id,
            name,
            description,
            org_id,
            created_at: now,
            updated_at: now,
        }
    }
}
