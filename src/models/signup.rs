use crate::db::DB;
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
    pub async fn create(signup: Payload) -> surrealdb::Result<Vec<Record>> {
        let db = DB.get().unwrap();
        db.create(TABLE).content(signup).await
    }

    pub async fn get_by_email(email: String) -> surrealdb::Result<Option<Signup>> {
        let db = DB.get().unwrap();
        let sql = format!("SELECT * FROM {} WHERE email = $email", TABLE);
        let mut response = db.query(sql).bind(("email", email)).await?;
        let list: Vec<Signup> = response.take(0)?;
        let signup = list.first().cloned();
        Ok(signup)
    }

    pub async fn get_by_uuid(id: String) -> surrealdb::Result<Option<Signup>> {
        let db = DB.get().unwrap();
        let sql = format!("SELECT * FROM {} WHERE uuid = $uuid", TABLE);
        let mut response = db.query(sql).bind(("uuid", id)).await?;
        let list: Vec<Signup> = response.take(0)?;
        let signup = list.first().cloned();
        Ok(signup)
    }

    pub async fn delete(id: String) -> surrealdb::Result<Option<Signup>> {
        let db = DB.get().unwrap();
        db.delete((TABLE, id)).await
    }
}
