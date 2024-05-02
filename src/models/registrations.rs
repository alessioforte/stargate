use crate::db::DB;
use crate::models::records::Record;
use serde::{Deserialize, Serialize};

use crate::models::records::thing_to_string;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewRegistration {
    pub email: String,
    pub uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registration {
    #[serde(deserialize_with = "thing_to_string")]
    pub id: String,
    pub email: String,
    pub uuid: String,
}

impl Registration {
    pub async fn create(registration: NewRegistration) -> surrealdb::Result<Vec<Record>> {
        let db = DB.get().unwrap();
        db.create("registrations").content(registration).await
    }

    pub async fn get_by_email(email: String) -> surrealdb::Result<Option<Registration>> {
        let db = DB.get().unwrap();
        let sql = "SELECT * FROM registrations WHERE email = $email";
        let mut response = db.query(sql).bind(("email", email)).await?;
        let registrations: Vec<Registration> = response.take(0)?;
        let registration = registrations.first().cloned();
        Ok(registration)
    }

    pub async fn get_by_uuid(id: String) -> surrealdb::Result<Option<Registration>> {
        let db = DB.get().unwrap();
        let sql = "SELECT * FROM registrations WHERE uuid = $uuid";
        let mut response = db.query(sql).bind(("uuid", id)).await?;
        let registrations: Vec<Registration> = response.take(0)?;
        let registration = registrations.first().cloned();
        Ok(registration)
    }

    pub async fn delete(id: String) -> surrealdb::Result<Option<Registration>> {
        let db = DB.get().unwrap();
        db.delete(("registrations", id)).await
    }
}
