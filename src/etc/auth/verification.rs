use crate::etc::{self, auth::subject::Subject, http::request::RequestExt};
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

use super::identity::VerifiedIdentity;

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

/// Verify an API key and return its persisted or cached effective identity.
pub async fn verify_api_key<R: RequestExt + ?Sized>(req: &R) -> Option<VerifiedIdentity> {
    let api_key = req.get_api_key()?;
    let hash_key = cached_hash_api_key(&api_key);
    let store = etc::store::use_store();
    let session = store.get::<Subject>(&hash_key).await.unwrap_or(None);

    if let Some(subject) = session {
        return VerifiedIdentity::api_key(subject);
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

        return VerifiedIdentity::api_key(subject);
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

async fn token_is_active(claims: &jwt::Claims) -> bool {
    match crate::act::token_revocation::is_revoked(claims).await {
        Ok(false) => true,
        Ok(true) => false,
        Err(e) => {
            error!("Failed to check token revocation state: {}", e);
            false
        }
    }
}

async fn verify_session_claims(claims: jwt::Claims) -> Option<VerifiedIdentity> {
    if !token_is_active(&claims).await {
        return None;
    }

    let store = etc::store::use_store();
    let sid = claims.sid.clone()?;
    match store.get::<Subject>(&sid).await {
        Ok(Some(subject)) => VerifiedIdentity::jwt(subject, sid, claims.auth_time),
        Ok(None) => None,
        Err(e) => {
            error!("Failed to get subject from the session store: {}", e);
            None
        }
    }
}

async fn verify_oauth_claims(claims: jwt::Claims) -> Option<VerifiedIdentity> {
    if !token_is_active(&claims).await {
        return None;
    }

    let sid = claims.sid.clone()?;
    let user_id = claims.sub_id.as_deref()?;
    let client_id = claims.azp.as_deref()?;
    let auth_time = claims.auth_time?;
    if claims.sub != user_id {
        return None;
    }

    let client = match crate::db::get_oauth_client_by_client_id(client_id).await {
        Ok(Some(client)) if client.enabled => client,
        Ok(_) => return None,
        Err(e) => {
            error!("Failed to get OAuth client: {}", e);
            return None;
        }
    };
    if let Some(audience) = claims.aud.as_deref()
        && !client.audiences.iter().any(|allowed| allowed == audience)
    {
        return None;
    }

    let session = match crate::act::sessions::get_active_session(&sid, user_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return None,
        Err(e) => {
            error!("Failed to resolve backing OAuth session: {}", e);
            return None;
        }
    };
    if !session.client_ids.iter().any(|linked| linked == client_id) {
        return None;
    }

    let subject = match etc::store::use_store().get::<Subject>(&sid).await {
        Ok(Some(subject)) if subject.id == user_id => subject,
        Ok(_) => return None,
        Err(e) => {
            error!("Failed to get OAuth subject from the session store: {}", e);
            return None;
        }
    };

    VerifiedIdentity::oauth(subject, sid, auth_time, claims.aud)
}

/// Verify a native session JWT or a user-delegated OAuth access token and bind
/// it to the current subject loaded from trusted session state.
pub async fn verify_bearer<R: RequestExt + ?Sized>(req: &R) -> Option<VerifiedIdentity> {
    let token = req.get_token()?;

    let jwt = etc::auth::jwt::jwt_config();
    if let Ok(claims) = jwt.validate_session_access_token(&token) {
        return verify_session_claims(claims).await;
    }

    let claims = jwt.validate_oauth_access_token(&token, None).ok()?;
    verify_oauth_claims(claims).await
}
