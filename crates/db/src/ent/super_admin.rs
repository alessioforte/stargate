use serde::{Deserialize, Serialize};

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuperAdmin {
    pub user_id: String,
    pub active: bool,
}

impl SuperAdmin {
    pub fn new(user_id: String) -> Self {
        SuperAdmin {
            user_id,
            active: true,
        }
    }
}
