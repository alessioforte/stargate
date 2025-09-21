use objectid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccount {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

impl ServiceAccount {
    pub fn new(name: String, description: Option<String>) -> Self {
        let id = ObjectId::new().unwrap().to_string();
        ServiceAccount {
            id,
            name,
            description,
        }
    }
}
