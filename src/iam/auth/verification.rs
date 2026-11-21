use super::identity::VerifiedIdentity;
use crate::{err::ErrorResponse, iam::auth::subject::Subject, web::request::RequestExt};
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
pub async fn verify_api_key<R: RequestExt + ?Sized>(
    req: &R,
) -> Result<Option<VerifiedIdentity>, ErrorResponse> {
    let Some(api_key) = req.get_api_key() else {
        return Ok(None);
    };
    // Hashing costs ~0.35 us for a 46-byte key (crates/pw/benches/api_key_hash.rs),
    // three orders of magnitude less than the subject read that follows
    // (docs/gateway-performance.md, "Request paths with cluster backends"). A
    // cache of cleartext keys would keep secrets, and the keys of anyone who
    // sends one, in memory for no measurable gain.
    let hash_key = pw::hash_api_key(&api_key);
    let store = crate::data::store::use_store();
    let cache_key = store::keys::api_key::subject(&hash_key);
    let session = store.get::<Subject>(&cache_key).await?;

    if let Some(subject) = session {
        return Ok(VerifiedIdentity::api_key(subject));
    }

    let result = crate::data::db::get_api_key_auth_by_hash(&hash_key)
        .await
        .map_err(ErrorResponse::internal)?;

    if let Some(auth) = result {
        if auth.api_key.revoked {
            return Ok(None);
        }

        let subject = Subject::from(auth);
        let ttl = Some(API_KEY_SUBJECT_TTL_SECS);
        match store.set(&cache_key, &subject, ttl).await {
            Ok(_) => (),
            Err(e) => {
                error!("Failed to store subject in the session store: {}", e);
            }
        }

        return Ok(VerifiedIdentity::api_key(subject));
    }

    Ok(None)
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

/// Purge all revoked keys even if a replica fails to acknowledge one.
pub async fn purge_revoked_api_keys(key_hashes: &[String]) -> store::StoreResult<()> {
    crate::data::store::without_security_budget(async {
        let mut result = Ok(());
        for hash in key_hashes {
            if let Err(error) =
                crate::data::store::delete_confirmed(&store::keys::api_key::subject(hash)).await
            {
                tracing::warn!(%error, "Failed to purge revoked API key");
                result = Err(error);
            }
        }
        result
    })
    .await
}

async fn verify_session_claims(
    claims: jwt::Claims,
) -> Result<Option<VerifiedIdentity>, ErrorResponse> {
    let Some(sid) = claims.sid.as_deref() else {
        return Ok(None);
    };
    if crate::iam::auth::revocation::is_revoked(&claims).await? {
        return Ok(None);
    }

    let subject = crate::iam::sess::subjects::read(sid)
        .await?;
    Ok(
        subject
            .and_then(|subject| VerifiedIdentity::jwt(subject, sid.to_owned(), claims.auth_time)),
    )
}

async fn verify_oauth_claims(
    claims: jwt::Claims,
) -> Result<Option<VerifiedIdentity>, ErrorResponse> {
    let (Some(sid), Some(user_id), Some(client_id), Some(auth_time)) = (
        claims.sid.as_deref(),
        claims.sub_id.as_deref(),
        claims.azp.as_deref(),
        claims.auth_time,
    ) else {
        return Ok(None);
    };
    if claims.sub != user_id || crate::iam::auth::revocation::is_revoked(&claims).await? {
        return Ok(None);
    }

    let Some(client) = crate::data::db::get_oauth_client_by_client_id(client_id)
        .await
        .map_err(ErrorResponse::internal)?
        .filter(|client| client.enabled)
    else {
        return Ok(None);
    };
    if let Some(audience) = claims.aud.as_deref()
        && !client.audiences.iter().any(|allowed| allowed == audience)
    {
        return Ok(None);
    }

    let Some(session) = crate::iam::sess::registry::active_token_session(&claims).await? else {
        return Ok(None);
    };
    if !session.client_ids.iter().any(|linked| linked == client_id) {
        return Ok(None);
    }
    let subject = crate::iam::sess::subjects::read(sid)
        .await?;
    Ok(subject
        .filter(|subject| subject.id == user_id)
        .and_then(|subject| {
            VerifiedIdentity::oauth(subject, sid.to_owned(), auth_time, claims.aud)
        }))
}

/// Verify a native session JWT or a user-delegated OAuth access token and bind
/// it to the current subject loaded from trusted session state.
pub async fn verify_bearer<R: RequestExt + ?Sized>(
    req: &R,
) -> Result<Option<VerifiedIdentity>, ErrorResponse> {
    let Some(token) = req.get_token() else {
        return Ok(None);
    };

    let jwt = super::jwt::jwt_config();
    if let Ok(claims) = jwt.validate_session_access_token(&token) {
        return verify_session_claims(claims).await;
    }

    let Ok(claims) = jwt.validate_oauth_access_token(&token, None) else {
        return Ok(None);
    };
    verify_oauth_claims(claims).await
}
