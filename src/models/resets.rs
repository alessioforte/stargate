use crate::db::DB;
use crate::models::record::thing_to_string;
use crate::models::record::Record;
use serde::{Deserialize, Serialize};

const RESOURCE: &str = "resets";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewPasswordReset {
    pub email: String,
    pub uuid: String,
    pub issued_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordReset {
    #[serde(deserialize_with = "thing_to_string")]
    pub id: String,
    pub email: String,
    pub uuid: String,
    pub issued_at: i64,
    pub expires_at: i64,
}

impl PasswordReset {
    pub async fn create(reset: NewPasswordReset) -> surrealdb::Result<Vec<Record>> {
        let db = DB.get().unwrap();
        db.create(RESOURCE).content(reset).await
    }

    pub async fn get_by_uuid(id: String) -> surrealdb::Result<Option<PasswordReset>> {
        let db = DB.get().unwrap();
        let sql = format!("SELECT * FROM {} WHERE uuid = $uuid", RESOURCE);
        let mut response = db.query(sql).bind(("uuid", id)).await?;
        let registrations: Vec<PasswordReset> = response.take(0)?;
        let registration = registrations.first().cloned();
        Ok(registration)
    }

    pub async fn delete(id: String) -> surrealdb::Result<Option<PasswordReset>> {
        let db = DB.get().unwrap();
        db.delete(("resets", id)).await
    }
}
