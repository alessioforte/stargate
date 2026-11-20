use chrono::Utc;
use store::{Store, StoreError};

fn key(jti: &str) -> String {
    store::keys::session::revoked_jti(jti)
}

fn ttl_until_exp(exp: usize) -> Option<u64> {
    let now = Utc::now().timestamp();
    let exp = i64::try_from(exp).ok()?;
    (exp > now).then_some((exp - now) as u64)
}

pub async fn is_revoked(claims: &jwt::Claims) -> Result<bool, StoreError> {
    let Some(jti) = claims.jti.as_deref() else {
        return Ok(false);
    };
    crate::data::store::use_store().exists(&key(jti)).await
}

pub async fn revoke_claims(claims: &jwt::Claims) -> Result<(), StoreError> {
    let store = crate::data::store::use_store();
    let mut jti_revoked = false;

    if let Some(jti) = claims.jti.as_deref()
        && let Some(ttl) = ttl_until_exp(claims.exp)
    {
        store.set(&key(jti), &true, Some(ttl)).await?;
        jti_revoked = true;
    }

    if (claims.typ.as_deref() == Some("refresh") || !jti_revoked)
        && let Some(sid) = claims.sid.as_deref()
    {
        store.delete(&store::keys::session::subject(sid)).await?;
    }

    Ok(())
}
