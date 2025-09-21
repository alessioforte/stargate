use objectid::ObjectId;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::str::FromStr;

pub enum SubjectType {
    User,
    ApiKey,
    ServiceAccount,
}

impl FromStr for SubjectType {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(SubjectType::User),
            "api_key" => Ok(SubjectType::ApiKey),
            "service_account" => Ok(SubjectType::ServiceAccount),
            _ => Err("Unknown subject type"),
        }
    }
}

impl SubjectType {
    pub fn as_str(&self) -> &str {
        match self {
            SubjectType::User => "user",
            SubjectType::ApiKey => "api_key",
            SubjectType::ServiceAccount => "service_account",
        }
    }
}

/// Represents a subject in the system, which can be a user, API key, or service account.
///
/// This struct is used to define the subjects that can be associated with actions in the access control system.
///
/// The `Subject` struct includes:
///
/// - `id`: A unique identifier for the subject, generated as an ObjectId.
///
/// - `kind`: The type of subject (e.g., "user", "api_key", "service_account").
///
/// - `subject_id`: The identifier of the subject (e.g., user ID, API key ID, service account ID).
///
/// - `attrs`: A flexible JSON object that can hold additional attributes related to the subject.
#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    pub id: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub sub_type: String,
    pub sub_id: String,
    pub attrs: JsonValue,
}

impl Subject {
    pub fn new(sub_type: SubjectType, sub_id: String, attrs: Option<JsonValue>) -> Self {
        let id = ObjectId::new().unwrap().to_string();

        Subject {
            id,
            sub_type: sub_type.as_str().to_string(),
            sub_id,
            attrs: attrs.unwrap_or(JsonValue::Object(serde_json::Map::new())),
        }
    }

    /// Get attrs as JsonValue
    pub fn get_attrs(&self) -> Result<JsonValue, serde_json::Error> {
        Ok(self.attrs.clone())
    }

    /// Set attrs from JsonValue
    pub fn set_attrs(&mut self, attrs: JsonValue) -> Result<JsonValue, serde_json::Error> {
        self.attrs = attrs;
        Ok(self.attrs.clone())
    }
}
