use crate::etc;
use db::{Transaction, ent::Subject};
use store::Store;

// TODO: maybe should return a Result instead of an Option
pub async fn validate_api_key(key: &str) -> Option<Subject> {
    let hash_key = apiks::hash_api_key(key);
    let store = etc::store::use_store();
    let session = store
        .get::<db::ent::Subject>(&hash_key)
        .await
        .unwrap_or(None);

    if let Some(subject) = session {
        return Some(subject);
    }

    // if is not found in the store try to find it in the database
    let service = etc::db::service();
    let result = match service.get_api_key_by_hash(&hash_key).await {
        Ok(api_key) => api_key,
        Err(e) => {
            log::error!("Failed to get api key: {}", e);
            return None;
        }
    };

    if let Some(api_key) = result {
        if api_key.revoked {
            return None;
        }

        let subject = match service.get_subject_by_id(&api_key.id).await {
            Ok(subject) => subject,
            Err(e) => {
                log::error!("Failed to get subject by id: {}", e);
                return None;
            }
        };

        // store the subject in the session store
        if let Some(subject) = subject {
            // let ttl = api_key.exp.map(|exp| exp as u64);
            let ttl = Some(3600);
            match store.set(&hash_key, &subject, ttl).await {
                Ok(_) => (),
                Err(e) => {
                    log::error!("Failed to store subject in the session store: {}", e);
                }
            }

            return Some(subject);
        }
    }

    None
}
