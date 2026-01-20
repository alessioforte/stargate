use crate::etc::{self, ext::RequestExt, sub::Subject};
use actix_web::HttpRequest;
use store::Store;
use tracing::error;

// TODO: maybe should return a Result instead of an Option
pub async fn verify_api_key(req: &HttpRequest) -> Option<Subject> {
    let api_key = match req.get_api_key() {
        Some(k) => k,
        None => {
            return None;
        }
    };
    let hash_key = pw::hash_api_key(&api_key);
    let store = etc::store::use_store();
    let session = store.get::<Subject>(&hash_key).await.unwrap_or(None);

    if let Some(subject) = session {
        return Some(subject);
    }

    // if is not found in the store try to find it in the database
    let result = match crate::db::get_api_key_by_hash(&hash_key).await {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to get api key: {}", e);
            return None;
        }
    };

    if let Some(api_key) = result {
        if api_key.revoked {
            return None;
        }

        let subject = Subject::from(api_key.clone());
        let ttl = Some(3600);
        match store.set(&hash_key, &subject, ttl).await {
            Ok(_) => (),
            Err(e) => {
                error!("Failed to store subject in the session store: {}", e);
            }
        }

        return Some(subject);
    }

    None
}

pub async fn verify_jwt(req: &HttpRequest) -> Option<Subject> {
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
    let session = match store.get::<Subject>(&sid).await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to get subject from the session store: {}", e);
            None
        }
    };

    session
}
