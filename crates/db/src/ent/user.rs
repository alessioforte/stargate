use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub account_id: String,
    pub email: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    pub attrs: Value,
}

impl User {
    pub fn new(account_id: String, email: String) -> Self {
        let id = ulid::Ulid::new().to_string();
        User {
            id,
            account_id,
            email,
            given_name: None,
            family_name: None,
            nickname: None,
            picture: None,
            phone_number: None,
            attrs: Value::Null,
        }
    }

    pub fn given_name(mut self, given_name: Option<String>) -> Self {
        self.given_name = given_name;
        self
    }

    pub fn family_name(mut self, family_name: Option<String>) -> Self {
        self.family_name = family_name;
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
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    pub attrs: Value,
}

impl Profile {
    pub fn new(email: String) -> Self {
        Profile {
            email,
            given_name: None,
            family_name: None,
            nickname: None,
            picture: None,
            phone_number: None,
            attrs: Value::Null,
        }
    }

    pub fn given_name(mut self, given_name: Option<String>) -> Self {
        self.given_name = given_name;
        self
    }

    pub fn family_name(mut self, family_name: Option<String>) -> Self {
        self.family_name = family_name;
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
