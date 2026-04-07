use crate::etc::store::use_store;
use store::Store;

const KEY_PREFIX: &str = "oauth_state";
const TTL_SECS: u64 = 300; // 5 minutes

pub async fn create_oauth_state() -> Result<String, store::StoreError> {
    let store = use_store();
    let state = ulid::Ulid::new().to_string();
    let key = format!("{}:{}", KEY_PREFIX, state);
    store.set(key.as_str(), &"1", Some(TTL_SECS)).await?;
    Ok(state)
}

pub async fn validate_oauth_state(state: &str) -> Result<bool, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, state);
    let exists = store.get::<String>(key.as_str()).await?.is_some();
    if exists {
        store.delete(key.as_str()).await?;
    }
    Ok(exists)
}
