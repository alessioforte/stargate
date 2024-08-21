use crate::db;
use crate::model;
use crate::models::record::thing_to_string;
use crate::models::record::Record;
use serde::{Deserialize, Serialize};

const RESOURCE: &str = "resets";

model! {
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PasswordReset {
        pub email: String,
        pub uuid: String,
        pub issued_at: i64,
        pub expires_at: i64,
    },
    {
        #[serde(deserialize_with = "thing_to_string")]
        pub id: String,
    }
}

impl PasswordReset {
    pub async fn save(reset: Payload) -> surrealdb::Result<Vec<Record>> {
        let db = db::connection().await?;
        db.create(RESOURCE).content(reset).await
    }

    pub async fn get_by_uuid(id: &str) -> surrealdb::Result<Option<PasswordReset>> {
        let db = db::connection().await?;
        let sql = format!("SELECT * FROM {} WHERE uuid = $uuid", RESOURCE);
        let mut response = db.query(sql).bind(("uuid", id)).await?;
        let registrations: Vec<PasswordReset> = response.take(0)?;
        let registration = registrations.first().cloned();
        Ok(registration)
    }

    pub async fn delete(id: &str) -> surrealdb::Result<Option<PasswordReset>> {
        let db = db::connection().await?;
        db.delete(("resets", id)).await
    }
}
