use crate::etc::store::use_store;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use store::{Store, StoreError};

const KEY_PREFIX: &str = "signup_requests";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSignupProfile {
    pub email: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    pub attrs: Value,
}

impl PendingSignupProfile {
    pub fn email_only(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            given_name: None,
            family_name: None,
            nickname: None,
            picture: None,
            phone_number: None,
            attrs: Value::Object(serde_json::Map::new()),
        }
    }
}

pub async fn create_signup_request(
    sid: &str,
    profile: &PendingSignupProfile,
) -> Result<(), store::StoreError> {
    let store = use_store();

    let key = format!("{}:{}", KEY_PREFIX, sid);
    let ttl = Some(60 * 60); // 60 minutes
    store.set(key.as_str(), profile, ttl).await
}

pub async fn get_signup_request(
    sid: &str,
) -> Result<Option<PendingSignupProfile>, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, sid);
    match store.get::<PendingSignupProfile>(key.as_str()).await {
        Ok(profile) => Ok(profile),
        Err(StoreError::DeserializationFailed(_)) => store
            .get::<String>(key.as_str())
            .await
            .map(|email| email.map(PendingSignupProfile::email_only)),
        Err(error) => Err(error),
    }
}

pub async fn delete_signup_request(sid: &str) -> Result<bool, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, sid);
    store.delete(key.as_str()).await
}
