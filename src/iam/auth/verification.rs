use super::identity::VerifiedIdentity;
use crate::{iam::auth::subject::Subject, web::request::RequestExt};
use store::Store;
use tracing::error;

/// How long a verified API key subject stays cached.
///
/// Attribute and organization writes purge the entry, so the TTL only bounds
/// the staleness of a change made outside those flows (a direct database
/// edit). A few minutes is enough to keep the cache useful without letting a
/// stale identity outlive a deliberate revocation by much.
const API_KEY_SUBJECT_TTL_SECS: u64 = 300;

/// Verify an API key and return its persisted or cached effective identity.
pub async fn verify_api_key<R: RequestExt + ?Sized>(req: &R) -> Option<VerifiedIdentity> {
    let api_key = req.get_api_key()?;
    // Hashing costs ~0.35 us for a 46-byte key (crates/pw/benches/api_key_hash.rs),
    // three orders of magnitude less than the subject read that follows
    // (docs/gateway-performance.md, "Request paths with cluster backends"). A
    // cache of cleartext keys would keep secrets, and the keys of anyone who
    // sends one, in memory for no measurable gain.
    let hash_key = pw::hash_api_key(&api_key);
    let store = crate::data::store::use_store();
    let cache_key = store::keys::api_key::subject(&hash_key);
    let session = store.get::<Subject>(&cache_key).await.unwrap_or(None);

    if let Some(subject) = session {
        return VerifiedIdentity::api_key(subject);
    }

    let result = match crate::data::db::get_api_key_auth_by_hash(&hash_key).await {
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
        let ttl = Some(API_KEY_SUBJECT_TTL_SECS);
        match store.set(&cache_key, &subject, ttl).await {
            Ok(_) => (),
            Err(e) => {
                error!("Failed to store subject in the session store: {}", e);
            }
        }

        return VerifiedIdentity::api_key(subject);
    }

    None
}

/// Drop the cached subject of one API key. Call after writing its attributes
/// or organization so the next request reads the new state instead of waiting
/// for the cache entry to expire. Best-effort: failures are logged and the
/// entry dies at its TTL.
pub async fn purge_api_key_subject(key_hash: &str) {
    if let Err(e) = crate::data::store::use_store()
        .delete(&store::keys::api_key::subject(key_hash))
        .await
    {
        error!("Failed to purge cached api key subject: {}", e);
    }
}

/// Drop the cached subjects of several API keys, for flows that delete keys in
/// bulk (deleting an owner or an organization).
pub async fn purge_api_key_subjects(key_hashes: &[String]) {
    for hash in key_hashes {
        purge_api_key_subject(hash).await;
    }
}

async fn token_is_active(claims: &jwt::Claims) -> bool {
    match crate::iam::auth::revocation::is_revoked(claims).await {
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

    let sid = claims.sid.clone()?;
    match crate::iam::sess::subjects::read(&sid).await {
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

    let client = match crate::data::db::get_oauth_client_by_client_id(client_id).await {
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

    let session = match crate::iam::sess::registry::active_token_session(&claims).await {
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

    let subject = match crate::iam::sess::subjects::read(&sid).await {
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

    let jwt = super::jwt::jwt_config();
    if let Ok(claims) = jwt.validate_session_access_token(&token) {
        return verify_session_claims(claims).await;
    }

    let claims = jwt.validate_oauth_access_token(&token, None).ok()?;
    verify_oauth_claims(claims).await
}
