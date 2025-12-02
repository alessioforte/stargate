use crate::etc::store::use_store;
use store::Store;

const KEY_PREFIX: &str = "email_verification_requests";

pub async fn create_email_verification_request(email: &str) -> Result<String, store::StoreError> {
    let store = use_store();
    let sid = ulid::Ulid::new().to_string();

    let key1 = format!("{}:{}", KEY_PREFIX, sid);
    let key2 = format!("{}:{}", KEY_PREFIX, email);
    let ttl = Some(60 * 60); // 60 minutes
    store.set(key1.as_str(), &email, ttl).await?;
    store.set(key2.as_str(), &sid, ttl).await?;
    Ok(sid)
}

pub async fn check_email_verification_request(
    email: &str,
) -> Result<Option<String>, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, email);
    store.get::<String>(key.as_str()).await
}

pub async fn get_email_verification_request(
    sid: &str,
) -> Result<Option<String>, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, sid);
    store.get::<String>(key.as_str()).await
}

pub async fn delete_email_verification_request(sid: &str) -> Result<(), store::StoreError> {
    let store = use_store();
    if let Some(email) = get_email_verification_request(sid).await? {
        let key1 = format!("{}:{}", KEY_PREFIX, sid);
        let key2 = format!("{}:{}", KEY_PREFIX, email);
        store.delete(key1.as_str()).await?;
        store.delete(key2.as_str()).await?;
    }
    Ok(())
}
