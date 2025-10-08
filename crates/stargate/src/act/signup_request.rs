use crate::etc::store::use_store;
use store::Store;
use uuid::Uuid;

const KEY_PREFIX: &str = "signup_requests";

pub async fn create_signup_request(email: &str) -> Result<String, store::StoreError> {
    let store = use_store();
    let uuid = Uuid::new_v4().to_string();

    let key = format!("{}:{}", KEY_PREFIX, uuid);
    let ttl = Some(60 * 60); // 60 minutes
    store.set(key.as_str(), &email, ttl).await?;
    Ok(uuid)
}

pub async fn get_signup_request(uuid: &str) -> Result<Option<String>, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, uuid);
    store.get::<String>(key.as_str()).await
}

pub async fn delete_signup_request(uuid: &str) -> Result<bool, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, uuid);
    store.delete(key.as_str()).await
}
