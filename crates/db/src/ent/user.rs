use objectid::ObjectId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub account_id: String,
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    pub attrs: Value,
}

impl User {
    pub fn new(account_id: String, email: String) -> Self {
        let id = ObjectId::new().unwrap().to_string();
        User {
            id,
            account_id,
            email,
            first_name: None,
            last_name: None,
            nickname: None,
            picture: None,
            phone_number: None,
            attrs: Value::Null,
        }
    }

    pub fn first_name(mut self, first_name: Option<String>) -> Self {
        self.first_name = first_name;
        self
    }

    pub fn last_name(mut self, last_name: Option<String>) -> Self {
        self.last_name = last_name;
        self
    }

    pub fn email(mut self, email: String) -> Self {
        self.email = email;
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

    pub fn attrs(mut self, attrs: Value) -> Self {
        self.attrs = attrs;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    pub attrs: Value,
}

impl Profile {
    pub fn new(email: String) -> Self {
        Profile {
            email,
            first_name: None,
            last_name: None,
            nickname: None,
            picture: None,
            phone_number: None,
            attrs: Value::Null,
        }
    }

    pub fn first_name(mut self, first_name: Option<String>) -> Self {
        self.first_name = first_name;
        self
    }

    pub fn last_name(mut self, last_name: Option<String>) -> Self {
        self.last_name = last_name;
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

    pub fn attrs(mut self, attrs: Value) -> Self {
        self.attrs = attrs;
        self
    }
}
