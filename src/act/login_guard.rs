use crate::etc::store::use_store;
use store::Store;

const KEY_PREFIX: &str = "login_attempts";
const MAX_ATTEMPTS: u32 = 5;
const LOCKOUT_SECS: u64 = 300; // 5 minutes

pub async fn check_login_allowed(key: &str) -> Result<bool, store::StoreError> {
    let store = use_store();
    let store_key = format!("{}:{}", KEY_PREFIX, key);
    let attempts: Option<u32> = store.get(&store_key).await?;
    Ok(attempts.unwrap_or(0) < MAX_ATTEMPTS)
}

pub async fn record_failed_attempt(key: &str) -> Result<u32, store::StoreError> {
    let store = use_store();
    let store_key = format!("{}:{}", KEY_PREFIX, key);
    let attempts: u32 = store.get(&store_key).await?.unwrap_or(0) + 1;
    store.set(&store_key, &attempts, Some(LOCKOUT_SECS)).await?;
    Ok(attempts)
}

pub async fn clear_attempts(key: &str) -> Result<(), store::StoreError> {
    let store = use_store();
    let store_key = format!("{}:{}", KEY_PREFIX, key);
    let _ = store.delete(&store_key).await;
    Ok(())
}

pub fn lockout_seconds() -> u64 {
    LOCKOUT_SECS
}
