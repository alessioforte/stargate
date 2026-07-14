use db::ent::{ApiKey, ApiKeyAuth, User};
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
    /// The identifier of the subject (e.g., user ID, API key ID, service account ID).
    pub id: String,

    /// The type of subject (e.g., "user", "api_key").
    #[serde(rename = "type")]
    pub sub_type: SubjectType,

    /// The organization ID associated with the subject.
    pub org_id: Option<String>,

    /// The subject's role in the organization (user subjects only).
    pub org_role: Option<String>,

    /// Additional attributes related to the subject.
    pub attrs: JsonValue,
}

impl Subject {
    pub fn new(id: String, sub_type: SubjectType, attrs: Option<JsonValue>) -> Self {
        Subject {
            id,
            sub_type,
            org_id: None,
            org_role: None,
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
        Subject::new(user.id, SubjectType::User, Some(user.attrs))
    }
}

impl From<ApiKey> for Subject {
    fn from(api_key: ApiKey) -> Self {
        Subject::new(api_key.id, SubjectType::ApiKey, Some(api_key.attrs))
    }
}

impl From<ApiKeyAuth> for Subject {
    fn from(auth: ApiKeyAuth) -> Self {
        let mut subject = Subject::from(auth.api_key);
        subject.org_id = auth.org_id;
        subject
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Sessions serialized before the multi-org fields existed must keep
    /// deserializing (org fields default to None).
    #[test]
    fn subject_deserializes_from_pre_multi_org_session() {
        let old = r#"{"id":"u1","type":"user","attrs":{"plan":"pro"}}"#;
        let subject: Subject = serde_json::from_str(old).expect("old session deserializes");

        assert_eq!(subject.id, "u1");
        assert_eq!(subject.sub_type, SubjectType::User);
        assert_eq!(subject.org_id, None);
        assert_eq!(subject.org_role, None);
        assert_eq!(subject.get_attr("plan"), Some(&json!("pro")));
    }

    #[test]
    fn subject_roundtrips_org_fields() {
        let mut subject = Subject::new("u1".to_string(), SubjectType::User, None);
        subject.org_id = Some("org_1".to_string());
        subject.org_role = Some("admin".to_string());

        let serialized = serde_json::to_string(&subject).unwrap();
        let restored: Subject = serde_json::from_str(&serialized).unwrap();

        assert_eq!(restored.org_id.as_deref(), Some("org_1"));
        assert_eq!(restored.org_role.as_deref(), Some("admin"));
    }

    #[test]
    fn api_key_auth_org_binding_flows_into_subject() {
        let api_key = db::ent::ApiKey::new("hash".to_string(), "label".to_string(), json!({}));
        let key_id = api_key.id.clone();

        let bound = Subject::from(ApiKeyAuth {
            api_key: api_key.clone(),
            org_id: Some("org_1".to_string()),
        });
        assert_eq!(bound.id, key_id);
        assert_eq!(bound.sub_type, SubjectType::ApiKey);
        assert_eq!(bound.org_id.as_deref(), Some("org_1"));

        let unbound = Subject::from(ApiKeyAuth {
            api_key,
            org_id: None,
        });
        assert_eq!(unbound.org_id, None);
    }
}
