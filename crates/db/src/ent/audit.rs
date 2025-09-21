use objectid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "action", rename_all = "lowercase")]
pub enum Action {
    Create,
    Update,
    Delete,
    Read,

    Login,
    Logout,
    PasswordChange,
}

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Audit {
    pub id: String,
    pub timestamp: i64,
    pub entity_type: String,
    pub entity_id: String,
    pub action: String,
    pub performed_by: String,
    pub details: Option<serde_json::Value>,
}

impl Audit {
    pub fn new(
        entity_type: String,
        entity_id: String,
        action: String,
        performed_by: String,
        details: Option<serde_json::Value>,
    ) -> Self {
        let id = ObjectId::new().unwrap().to_string();
        let timestamp = chrono::Utc::now().timestamp();
        Audit {
            id,
            entity_type,
            entity_id,
            action,
            timestamp,
            performed_by,
            details,
        }
    }
}
