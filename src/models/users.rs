use serde::{Deserialize, Serialize};

use crate::db::DB;
use crate::models::records::{thing_to_string, Record};

#[derive(Debug, Serialize, Deserialize)]
pub struct NewUser {
    pub name: String,
    pub email: String,
    pub password: String,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    // pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    #[serde(deserialize_with = "thing_to_string")]
    pub id: String,
    pub name: String,
    pub email: String,
    pub password: String,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    pub refresh_token: Option<String>,
}

impl User {
    pub async fn create(user: NewUser) -> surrealdb::Result<Vec<Record>> {
        DB.create("users").content(user).await
    }

    pub async fn get_all() -> surrealdb::Result<Vec<User>> {
        DB.select("users").await
    }

    pub async fn get(id: String) -> surrealdb::Result<Option<User>> {
        DB.select(("users", id)).await
    }

    pub async fn get_by_email(email: String) -> surrealdb::Result<Option<User>> {
        let sql = format!("SELECT * FROM users WHERE email = $email");
        let mut response = DB.query(sql).bind(("email", email)).await?;
        let users: Vec<User> = response.take(0)?;
        let user = users.first().cloned();
        Ok(user)
    }

    pub async fn delete(id: String) -> surrealdb::Result<Option<User>> {
        DB.delete(("users", id)).await
    }
}
