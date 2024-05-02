use crate::db::DB;
use crate::models::records::Record;
use serde::{Deserialize, Serialize};

use crate::models::records::thing_to_string;

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
        db.create("resets").content(reset).await
    }

    pub async fn get_by_uuid(id: String) -> surrealdb::Result<Option<PasswordReset>> {
        let db = DB.get().unwrap();
        let sql = "SELECT * FROM resets WHERE uuid = $uuid";
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
