use crate::db::DB;
use crate::models::record::{thing_to_string, Record};
use serde::{Deserialize, Serialize};

const RESOURCE: &str = "users";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewUser {
    pub name: String,
    pub email: String,
    pub password: String,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    #[serde(deserialize_with = "thing_to_string")]
    pub id: String,
    pub name: String,
    pub email: String,
    // #[serde(skip)]
    pub password: String,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    #[serde(deserialize_with = "thing_to_string")]
    pub id: String,
    pub name: String,
    pub email: String,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
}

impl User {
    pub async fn create(user: NewUser) -> surrealdb::Result<Vec<Record>> {
        let db = DB.get().unwrap();
        db.create(RESOURCE).content(user).await
    }

    pub async fn get_all() -> surrealdb::Result<Vec<User>> {
        let db = DB.get().unwrap();
        db.select(RESOURCE).await
    }

    pub async fn get(id: String) -> surrealdb::Result<Option<User>> {
        let db = DB.get().unwrap();
        db.select((RESOURCE, id)).await
    }

    pub async fn change_password(id: String, password: String) -> surrealdb::Result<Option<User>> {
        let db = DB.get().unwrap();
        let sql = format!("UPDATE {}:{} SET password = $password", RESOURCE, id);
        let mut response = db.query(sql).bind(("password", password)).await?;
        let users: Vec<User> = response.take(0)?;
        let user = users.first().cloned();
        Ok(user)
    }

    pub async fn get_by_email(email: String) -> surrealdb::Result<Option<User>> {
        let db = DB.get().unwrap();
        let sql = format!("SELECT * FROM {} WHERE email = $email", RESOURCE);
        let mut response = db.query(sql).bind(("email", email)).await?;
        let users: Vec<User> = response.take(0)?;
        let user = users.first().cloned();
        Ok(user)
    }

    pub async fn get_by_nickname(nickname: String) -> surrealdb::Result<Option<User>> {
        let db = DB.get().unwrap();
        let sql = format!("SELECT * FROM {} WHERE nickname = $nickname", RESOURCE);
        let mut response = db.query(sql).bind(("nickname", nickname)).await?;
        let users: Vec<User> = response.take(0)?;
        let user = users.first().cloned();
        Ok(user)
    }

    pub async fn delete(id: String) -> surrealdb::Result<Option<User>> {
        let db = DB.get().unwrap();
        db.delete((RESOURCE, id)).await
    }
}
