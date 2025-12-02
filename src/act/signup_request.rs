use crate::etc::store::use_store;
use store::Store;

const KEY_PREFIX: &str = "signup_requests";

pub async fn create_signup_request(email: &str) -> Result<String, store::StoreError> {
    let store = use_store();
    let sid = ulid::Ulid::new().to_string();

    let key = format!("{}:{}", KEY_PREFIX, sid);
    let ttl = Some(60 * 60); // 60 minutes
    store.set(key.as_str(), &email, ttl).await?;
    Ok(sid)
}

pub async fn get_signup_request(sid: &str) -> Result<Option<String>, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, sid);
    store.get::<String>(key.as_str()).await
}

pub async fn delete_signup_request(sid: &str) -> Result<bool, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, sid);
    store.delete(key.as_str()).await
}
