use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub attrs: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Organization {
    pub fn new(name: String, description: Option<String>) -> Self {
        let id = ulid::Ulid::new().to_string();
        let now = Utc::now();
        Organization {
            id,
            name,
            description,
            attrs: Value::Object(serde_json::Map::new()),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn attrs(mut self, attrs: Value) -> Self {
        self.attrs = attrs;
        self
    }
}
