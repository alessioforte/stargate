use chrono::Utc;
use store::{Store, StoreError};

const KEY_PREFIX: &str = "oauth_revoked_jti";

fn key(jti: &str) -> String {
    format!("{KEY_PREFIX}:{jti}")
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
    crate::etc::store::use_store().exists(&key(jti)).await
}

pub async fn revoke_claims(claims: &jwt::Claims) -> Result<(), StoreError> {
    let store = crate::etc::store::use_store();
    let mut jti_revoked = false;

    if let Some(jti) = claims.jti.as_deref()
        && let Some(ttl) = ttl_until_exp(claims.exp)
    {
        store.set(&key(jti), &true, Some(ttl)).await?;
        jti_revoked = true;
    }

    if claims.typ.as_deref() == Some("refresh") || !jti_revoked {
        if let Some(sid) = claims.sid.as_deref() {
            store.delete(sid).await?;
        }
    }

    Ok(())
}
