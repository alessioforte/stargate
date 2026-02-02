use db::ent::{ApiKey, User};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::str::FromStr;

/// Session subject type
///
/// Should be matched with the subject type in the access control system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SubjectType {
    User,
    ApiKey,
}

impl FromStr for SubjectType {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(SubjectType::User),
            "api_key" => Ok(SubjectType::ApiKey),
            _ => Err("Unknown subject type"),
        }
    }
}

impl SubjectType {
    pub fn as_str(&self) -> &str {
        match self {
            SubjectType::User => "user",
            SubjectType::ApiKey => "api_key",
        }
    }
}

/// Represents a subject in the system, which can be a user, API key, or service account.
///
/// This struct is used to define the subjects that can be associated with actions in the access control system.
///
/// The `Subject` struct includes:
///
/// - `id`: The identifier of the subject (e.g., user ID, API key ID, service account ID).
///
/// - `sub_type`: The type of subject (e.g., "user", "api_key").
///
/// - `attrs`: A flexible JSON object that can hold additional attributes related to the subject.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    #[serde(rename = "type")]
    pub id: String,
    pub account_id: String,
    pub sub_type: SubjectType,
    pub attrs: JsonValue,
}

impl Subject {
    pub fn new(
        id: String,
        account_id: String,
        sub_type: SubjectType,
        attrs: Option<JsonValue>,
    ) -> Self {
        Subject {
            id,
            account_id,
            sub_type,
            attrs: attrs.unwrap_or(JsonValue::Object(serde_json::Map::new())),
        }
    }

    /// Get attribute by key
    pub fn get_attr(&self, key: &str) -> Option<&JsonValue> {
        self.attrs.get(key)
    }
}

impl From<User> for Subject {
    fn from(user: User) -> Self {
        Subject::new(
            user.id,
            user.account_id,
            SubjectType::User,
            Some(user.attrs),
        )
    }
}

impl From<ApiKey> for Subject {
    fn from(api_key: ApiKey) -> Self {
        Subject::new(
            api_key.id,
            api_key.account_id,
            SubjectType::ApiKey,
            Some(api_key.attrs),
        )
    }
}
