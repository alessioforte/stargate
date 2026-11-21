use crate::data::store::{delete_confirmed, set_confirmed};
use chrono::Utc;
use store::{Store, StoreError, StoreResult};

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

/// A replication shortfall still leaves the write applied on the primary.
#[derive(Default)]
pub(crate) struct RevocationConfirmations(Option<StoreError>);

impl RevocationConfirmations {
    pub(crate) fn record<T>(&mut self, result: StoreResult<T>) -> StoreResult<Option<T>> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error @ StoreError::ReplicationShortfall { .. }) => {
                self.0.get_or_insert(error);
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn finish<T>(self, value: T) -> StoreResult<T> {
        self.0.map_or(Ok(value), Err)
    }
}

pub async fn revoke_claims(claims: &jwt::Claims) -> Result<(), StoreError> {
    let mut confirmations = RevocationConfirmations::default();
    let mut jti_revoked = false;

    if let Some(jti) = claims.jti.as_deref()
        && let Some(ttl) = ttl_until_exp(claims.exp)
    {
        confirmations.record(set_confirmed(&key(jti), &true, ttl).await)?;
        jti_revoked = true;
    }

    if (claims.typ.as_deref() == Some("refresh") || !jti_revoked)
        && let Some(sid) = claims.sid.as_deref()
    {
        confirmations.record(delete_confirmed(&store::keys::session::subject(sid)).await)?;
    }

    confirmations.finish(())
}
