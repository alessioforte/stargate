use crate::db;
use crate::ent::record::{thing_to_string, Record};
use crate::model;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

const RESOURCE: &str = "users";

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
struct UserAuth {
    user_id: String,
    password: String,
    timestamp: i64,
}

model! {
    #[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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
    pub fn new() -> Self {
        User {
            id: "".to_string(),
            name: "".to_string(),
            email: "".to_string(),
            password: None,
            nickname: None,
            picture: None,
            phone_number: None,
        }
    }

    pub fn name(mut self, name: String) -> Self {
        self.name = name;
        self
    }

    pub fn email(mut self, email: String) -> Self {
        self.email = email;
        self
    }

    pub fn password(mut self, password: Option<String>) -> Self {
        self.password = password;
        self
    }

    pub fn nickname(mut self, nickname: Option<String>) -> Self {
        self.nickname = nickname;
        self
    }

    pub fn picture(mut self, picture: Option<String>) -> Self {
        self.picture = picture;
        self
    }

    pub fn phone_number(mut self, phone_number: Option<String>) -> Self {
        self.phone_number = phone_number;
        self
    }

    pub async fn list() -> surrealdb::Result<Vec<Profile>> {
        let db = db::connection().await?;
        db.select(RESOURCE).await
    }

    pub async fn get(id: &str) -> surrealdb::Result<Option<User>> {
        let db = db::connection().await?;
        db.select((RESOURCE, id)).await
    }

    pub async fn save(mut self) -> surrealdb::Result<User> {
        let db = db::connection().await?;
        let payload = Payload {
            name: self.name.clone(),
            email: self.email.clone(),
            password: self.password.clone(),
            nickname: self.nickname.clone(),
            picture: self.picture.clone(),
            phone_number: self.phone_number.clone(),
        };
        let records: Result<Option<Record>, surrealdb::Error> =
            db.create(RESOURCE).content(payload).await;
        match records {
            Ok(records) => {
                let id = records.unwrap().id;
                self.id = id;
                Ok(self)
            }
            Err(e) => Err(e),
        }
    }

    pub async fn update(self) -> surrealdb::Result<Option<User>> {
        let db = db::connection().await?;
        let payload = Payload {
            name: self.name.clone(),
            email: self.email.clone(),
            password: self.password.clone(),
            nickname: self.nickname.clone(),
            picture: self.picture.clone(),
            phone_number: self.phone_number.clone(),
        };
        db.update((RESOURCE, self.id.clone()))
            .content(payload)
            .await
    }

    pub async fn change_password(id: &str, password: &str) -> surrealdb::Result<Option<User>> {
        let db = db::connection().await?;
        let sql = format!("UPDATE {}:{} SET password = $password", RESOURCE, id);
        let mut response = db
            .query(sql)
            .bind(("password", password.to_string()))
            .await?;
        let users: Vec<User> = response.take(0)?;
        let user = users.first().cloned();
        Ok(user)
    }

    pub async fn get_by_email(email: &str) -> surrealdb::Result<Option<User>> {
        let db = db::connection().await?;
        let sql = format!("SELECT * FROM {} WHERE email = $email", RESOURCE);
        let mut response = db.query(sql).bind(("email", email.to_string())).await?;
        let users: Vec<User> = response.take(0)?;
        let user = users.first().cloned();
        Ok(user)
    }

    pub async fn get_by_nickname(nickname: &str) -> surrealdb::Result<Option<User>> {
        let db = db::connection().await?;
        let sql = format!("SELECT * FROM {} WHERE nickname = $nickname", RESOURCE);
        let mut response = db
            .query(sql)
            .bind(("nickname", nickname.to_string()))
            .await?;
        let users: Vec<User> = response.take(0)?;
        let user = users.first().cloned();
        Ok(user)
    }

    pub async fn get_by_username(username: &str) -> surrealdb::Result<Option<User>> {
        let db = db::connection().await?;
        let sql = format!(
            "SELECT * FROM {} WHERE email = $username OR nickname = $username",
            RESOURCE
        );
        let mut response = db
            .query(sql)
            .bind(("username", username.to_string()))
            .await?;
        let users: Vec<User> = response.take(0)?;
        let user = users.first().cloned();
        Ok(user)
    }

    pub async fn delete(id: &str) -> surrealdb::Result<Option<User>> {
        let db = db::connection().await?;
        db.delete((RESOURCE, id)).await
    }
}
