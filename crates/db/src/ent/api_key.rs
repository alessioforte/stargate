use objectid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "owner_type", rename_all = "lowercase")]
pub enum OwnerType {
    User,
    ServiceAccount,
}

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub owner: String,
    pub owner_type: OwnerType,
    pub key_hash: String,
    pub label: Option<String>,
    pub revoked: bool,
    pub exp: Option<i64>,
}

impl ApiKey {
    pub fn new(
        key_hash: String,
        owner: String,
        owner_type: OwnerType,
        label: Option<String>,
        exp: Option<i64>,
    ) -> Self {
        let id = ObjectId::new().unwrap().to_string();
        ApiKey {
            id,
            owner,
            owner_type,
            key_hash,
            label,
            revoked: false,
            exp,
        }
    }
}
