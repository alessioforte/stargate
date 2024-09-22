use crate::db;
use crate::model;
use crate::models::record::thing_to_string;
use crate::models::record::Record;
use serde::{Deserialize, Serialize};

const TABLE: &str = "signups";

model! {
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Signup {
        pub email: String,
        pub uuid: String,
    },
    {
        #[serde(deserialize_with = "thing_to_string")]
        pub id: String,
    }
}

impl Signup {
    pub async fn save(signup: Payload) -> surrealdb::Result<Option<Record>> {
        let db = db::connection().await?;
        db.create(TABLE).content(signup).await
    }

    pub async fn get_by_email(email: &str) -> surrealdb::Result<Option<Signup>> {
        let db = db::connection().await?;
        let sql = format!("SELECT * FROM {} WHERE email = $email", TABLE);
        let mut response = db.query(sql).bind(("email", email.to_string())).await?;
        let list: Vec<Signup> = response.take(0)?;
        let signup = list.first().cloned();
        Ok(signup)
    }

    pub async fn get_by_uuid(id: &str) -> surrealdb::Result<Option<Signup>> {
        let db = db::connection().await?;
        let sql = format!("SELECT * FROM {} WHERE uuid = $uuid", TABLE);
        let mut response = db.query(sql).bind(("uuid", id.to_string())).await?;
        let list: Vec<Signup> = response.take(0)?;
        let signup = list.first().cloned();
        Ok(signup)
    }

    pub async fn delete(id: &str) -> surrealdb::Result<Option<Signup>> {
        let db = db::connection().await?;
        db.delete((TABLE, id)).await
    }
}
