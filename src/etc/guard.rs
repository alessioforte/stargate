use crate::etc::{self, ext::RequestExt, sub::Subject};
use lru::LruCache;
use std::cell::RefCell;
use std::num::NonZeroUsize;
use store::Store;
use tracing::error;

const HASH_CACHE_MAX_SIZE: usize = 256;

thread_local! {
    static HASH_CACHE: RefCell<LruCache<Box<str>, Box<str>>> =
        RefCell::new(LruCache::new(NonZeroUsize::new(HASH_CACHE_MAX_SIZE).unwrap()));
}

fn cached_hash_api_key(api_key: &str) -> String {
    HASH_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(hash) = cache.get(api_key) {
            return hash.to_string();
        }
        let hash = pw::hash_api_key(api_key);
        cache.put(Box::from(api_key), Box::from(hash.as_str()));
        hash
    })
}

/// Verify API key and return subject if valid from the session store
pub async fn verify_api_key<R: RequestExt + ?Sized>(req: &R) -> Option<Subject> {
    let api_key = req.get_api_key()?;
    let hash_key = cached_hash_api_key(&api_key);
    let store = etc::store::use_store();
    let session = store.get::<Subject>(&hash_key).await.unwrap_or(None);

    if let Some(subject) = session {
        return Some(subject);
    }

    let result = match crate::db::get_api_key_auth_by_hash(&hash_key).await {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to get api key: {}", e);
            return None;
        }
    };

    if let Some(auth) = result {
        if auth.api_key.revoked {
            return None;
        }

        let subject = Subject::from(auth);
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

/// Remove cached auth subjects for the given API key hashes. Call after
/// revoking keys so they stop authenticating immediately instead of when
/// the cache entry expires. Best-effort: failures are logged and the cache
/// entry dies at its TTL.
pub async fn purge_api_key_subjects(key_hashes: &[String]) {
    let store = etc::store::use_store();
    for hash in key_hashes {
        if let Err(e) = store.delete(hash).await {
            error!("Failed to purge cached api key subject: {}", e);
        }
    }
}

/// Verify JWT token and return subject if valid from the session store
pub async fn verify_jwt<R: RequestExt + ?Sized>(req: &R) -> Option<Subject> {
    let token = req.get_token()?;

    let jwt = etc::jwt::jwt_config();
    let claims = match jwt.validate_session_access_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return None;
        }
    };

    match crate::act::token_revocation::is_revoked(&claims).await {
        Ok(false) => {}
        Ok(true) => return None,
        Err(e) => {
            error!("Failed to check token revocation state: {}", e);
            return None;
        }
    }

    let store = etc::store::use_store();
    let sid = claims.sid.clone().unwrap_or_default();
    match store.get::<Subject>(&sid).await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to get subject from the session store: {}", e);
            None
        }
    }
}
