use crate::etc::store::use_store;
use store::Store;
use uuid::Uuid;

const KEY_PREFIX: &str = "change_password_requests";

pub async fn create_change_password_request(email: &str) -> Result<String, store::StoreError> {
    let store = use_store();
    let uuid = Uuid::new_v4().to_string();

    let key = format!("{}:{}", KEY_PREFIX, email);
    let ttl = Some(60 * 60); // 1 hour in seconds
    store.set(key.as_str(), &uuid, ttl).await?;
    Ok(uuid)
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
