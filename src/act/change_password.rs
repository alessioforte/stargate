use crate::etc::store::use_store;
use store::Store;
use uuid::Uuid;

const KEY_PREFIX: &str = "change_password_requests";

pub async fn create_change_password_request(email: &str) -> Result<String, store::StoreError> {
    let store = use_store();
    let uuid = Uuid::new_v4().to_string();

    let key = format!("{}:{}", KEY_PREFIX, uuid);
    let ttl = Some(60); // 1 minute
    store.set(key.as_str(), &email, ttl).await?;
    Ok(uuid)
}

pub async fn get_change_password_request(uuid: &str) -> Result<Option<String>, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, uuid);
    store.get::<String>(key.as_str()).await
}
