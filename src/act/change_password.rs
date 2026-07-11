use crate::etc::store::use_store;
use store::Store;

const KEY_PREFIX: &str = "change_password_requests";

/// Lifetime of a change-password request (and the reset tokens minted for it).
pub const CHANGE_PASSWORD_REQUEST_TTL_SECS: u64 = 60 * 60;

pub async fn create_change_password_request(email: &str) -> Result<String, store::StoreError> {
    let store = use_store();
    let sid = ulid::Ulid::new().to_string();

    let key = format!("{}:{}", KEY_PREFIX, email);
    store
        .set(key.as_str(), &sid, Some(CHANGE_PASSWORD_REQUEST_TTL_SECS))
        .await?;
    Ok(sid)
}

pub async fn get_change_password_request(email: &str) -> Result<Option<String>, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, email);
    store.get::<String>(key.as_str()).await
}

pub async fn delete_change_password_request(email: &str) -> Result<(), store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, email);
    store.delete(key.as_str()).await?;
    Ok(())
}
