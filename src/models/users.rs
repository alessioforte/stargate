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
}

impl User {
    pub async fn create(user: NewUser) -> surrealdb::Result<Vec<Record>> {
        let db = DB.get().unwrap();
        db.create("users").content(user).await
    }

    pub async fn get_all() -> surrealdb::Result<Vec<User>> {
        let db = DB.get().unwrap();
        db.select("users").await
    }

    pub async fn get(id: String) -> surrealdb::Result<Option<User>> {
        let db = DB.get().unwrap();
        db.select(("users", id)).await
    }

    pub async fn change_password(id: String, password: String) -> surrealdb::Result<Option<User>> {
        let db = DB.get().unwrap();
        let sql = format!("UPDATE users:{} SET password = $password", id);
        let mut response = db.query(sql).bind(("password", password)).await?;
        let users: Vec<User> = response.take(0)?;
        let user = users.first().cloned();
        Ok(user)
    }

    pub async fn get_by_email(email: String) -> surrealdb::Result<Option<User>> {
        let db = DB.get().unwrap();
        let sql = format!("SELECT * FROM users WHERE email = $email");
        let mut response = db.query(sql).bind(("email", email)).await?;
        let users: Vec<User> = response.take(0)?;
        let user = users.first().cloned();
        Ok(user)
    }

    pub async fn delete(id: String) -> surrealdb::Result<Option<User>> {
        let db = DB.get().unwrap();
        db.delete(("users", id)).await
    }
}
