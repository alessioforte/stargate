use serde::{Deserialize, Serialize};

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccount {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub org_id: Option<String>,
}

impl ServiceAccount {
    pub fn new(name: String, description: Option<String>, org_id: Option<String>) -> Self {
        let id = ulid::Ulid::new().to_string();
        ServiceAccount {
            id,
            name,
            description,
            org_id,
        }
    }
}
