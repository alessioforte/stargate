use crate::etc::{self, ext::RequestExt};
use actix_web::HttpRequest;
use db::Transaction;
use store::Store;

// TODO: maybe should return a Result instead of an Option
pub async fn verify_api_key(req: &HttpRequest) -> Option<etc::sub::Subject> {
    let api_key = match req.get_api_key() {
        Some(k) => k,
        None => {
            return None;
        }
    };
    let hash_key = pw::hash_api_key(&api_key);
    let store = etc::store::use_store();
    let session = store
        .get::<etc::sub::Subject>(&hash_key)
        .await
        .unwrap_or(None);

    if let Some(subject) = session {
        return Some(subject);
    }

    // if is not found in the store try to find it in the database
    let service = etc::db::service();
    let result = match service.get_api_key_by_hash(&hash_key).await {
        Ok(v) => v,
        Err(e) => {
            log::error!("Failed to get api key: {}", e);
            return None;
        }
    };

    if let Some(api_key) = result {
        if api_key.revoked {
            return None;
        }

        let subject = etc::sub::Subject::from(api_key.clone());
        let ttl = Some(3600);
        match store.set(&hash_key, &subject, ttl).await {
            Ok(_) => (),
            Err(e) => {
                log::error!("Failed to store subject in the session store: {}", e);
            }
        }

        return Some(subject);
    }

    None
}

pub async fn verify_jwt(req: &HttpRequest) -> Option<etc::sub::Subject> {
    let token = match req.get_token() {
        Some(t) => t,
        None => {
            return None;
        }
    };

    let jwt = etc::jwt::jwt_config();
    let claims = match jwt.validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return None;
        }
    };

    // Get subject from session store
    let store = etc::store::use_store();
    let sid = claims.sid.clone().unwrap_or_default();
    let session = store.get::<etc::sub::Subject>(&sid).await;
    println!("Session: {:?}", session);

    if let Ok(Some(subject)) = session {
        return Some(subject);
    }

    None
}
