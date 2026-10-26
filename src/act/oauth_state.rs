use crate::data::store::use_store;
use serde::{Deserialize, Serialize};
use store::Store;

const KEY_PREFIX: &str = "oauth_state";
const TTL_SECS: u64 = 300; // 5 minutes

/// What a social sign-in `state` was issued for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthState {
    /// Set for sign-ins a browser started at `/oauth/{provider}/start`;
    /// `None` for states handed to API clients (`POST /oauth/state`).
    pub browser: Option<BrowserSignIn>,
}

/// A sign-in the browser started, bound to it by a cookie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSignIn {
    /// The provider it started with (`google`, `github`).
    pub provider: String,
    /// Where the browser goes once signed in.
    pub return_to: Option<String>,
}

pub async fn create_oauth_state(record: &OAuthState) -> Result<String, store::StoreError> {
    let store = use_store();
    let state = ulid::Ulid::generate().to_string();
    let key = format!("{}:{}", KEY_PREFIX, state);
    store.set(key.as_str(), record, Some(TTL_SECS)).await?;
    Ok(state)
}

/// The record `state` was issued with, consumed so each state works once.
pub async fn take_oauth_state(state: &str) -> Result<Option<OAuthState>, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, state);
    let record = store.get::<OAuthState>(key.as_str()).await?;
    if record.is_some() {
        store.delete(key.as_str()).await?;
    }
    Ok(record)
}
