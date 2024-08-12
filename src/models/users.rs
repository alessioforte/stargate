use crate::db::DB;
use crate::model;
use crate::models::record::{thing_to_string, Record};
use serde::{Deserialize, Serialize};

const RESOURCE: &str = "users";

model! {
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct User {
        pub name: String,
        pub email: String,
        pub password: Option<String>,
        pub nickname: Option<String>,
        pub picture: Option<String>,
        pub phone_number: Option<String>,
    },
    {
        #[serde(deserialize_with = "thing_to_string")]
        pub id: String,
    }
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
    pub async fn create(user: Payload) -> surrealdb::Result<Record> {
        let db = DB.get().unwrap();
        let records = db.create(RESOURCE).content(user).await;
        match records {
            Ok(mut records) => Ok(records.pop().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub async fn update(id: String, user: Payload) -> surrealdb::Result<Option<User>> {
        let db = DB.get().unwrap();
        let sql = format!(
            "UPDATE {}:{} SET name = $name, email = $email, nickname = $nickname, picture = $picture, phone_number = $phone_number",
            RESOURCE, id
        );
        let mut response = db
            .query(sql)
            .bind(("name", user.name))
            .bind(("email", user.email))
            .bind(("nickname", user.nickname))
            .bind(("picture", user.picture))
            .bind(("phone_number", user.phone_number))
            .await?;
        let users: Vec<User> = response.take(0)?;
        let user = users.first().cloned();
        Ok(user)
    }

    pub fn password(&self) -> String {
        self.password.clone().unwrap_or_else(|| "".to_string())
    }

    pub async fn get_all() -> surrealdb::Result<Vec<Profile>> {
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
