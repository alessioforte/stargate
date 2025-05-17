use crate::db;
use crate::ent::record::{thing_to_string, Record};
use crate::model;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

const RESOURCE: &str = "passwords";

model! {
    #[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
    #[serde(rename_all = "camelCase")]
    pub struct Password {
        user_id: String,
        password: String,
        timestamp: i64,
    },
    {
        #[serde(deserialize_with = "thing_to_string")]
        pub id: String,
    }
}

impl Password {
    pub fn new(user_id: String, password: String) -> Payload {
        Payload {
            user_id,
            password,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    pub async fn save(self) -> surrealdb::Result<Option<Record>> {
        let db = db::connection().await?;
        db.create(RESOURCE).content(self).await
    }

    pub async fn get_by_user_id(user_id: &str) -> surrealdb::Result<Option<Password>> {
        let db = db::connection().await?;
        let sql = format!("SELECT * FROM {} WHERE user_id = $user_id AND timestamp = (SELECT MAX(timestamp) FROM {} WHERE user_id = $user_id)", RESOURCE, RESOURCE);
        let mut response = db.query(sql).bind(("user_id", user_id.to_string())).await?;
        let passwords: Vec<Password> = response.take(0)?;
        let password = passwords.first().cloned();
        Ok(password)
    }
}
