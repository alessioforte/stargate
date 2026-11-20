use crate::data::store::use_store;
use crate::iam::oidc::continuation::LoginContinuation;
use serde::{Deserialize, Serialize};
use store::Store;

const KEY_PREFIX: &str = "change_password_requests";

/// Lifetime of a change-password request (and the reset tokens minted for it).
pub const CHANGE_PASSWORD_REQUEST_TTL_SECS: u64 = 60 * 60;

/// A pending reset: the id its tokens carry, and the app the reset started
/// from, so the link works in another tab too.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub sid: String,
    #[serde(default)]
    pub continuation: Option<LoginContinuation>,
}

pub async fn create_change_password_request(
    email: &str,
    continuation: Option<LoginContinuation>,
) -> Result<String, store::StoreError> {
    let store = use_store();
    let sid = ulid::Ulid::generate().to_string();

    let key = format!("{}:{}", KEY_PREFIX, email);
    let request = ChangePasswordRequest {
        sid: sid.clone(),
        continuation,
    };
    store
        .set(
            key.as_str(),
            &request,
            Some(CHANGE_PASSWORD_REQUEST_TTL_SECS),
        )
        .await?;
    Ok(sid)
}

pub async fn get_change_password_request(
    email: &str,
) -> Result<Option<ChangePasswordRequest>, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, email);
    store.get::<ChangePasswordRequest>(key.as_str()).await
}

/// Keep the pending request but point it at the app the newest link was
/// requested from.
pub async fn set_continuation(
    email: &str,
    continuation: LoginContinuation,
) -> Result<(), store::StoreError> {
    let Some(mut request) = get_change_password_request(email).await? else {
        return Ok(());
    };
    if request.continuation.as_ref() == Some(&continuation) {
        return Ok(());
    }
    request.continuation = Some(continuation);
    let key = format!("{}:{}", KEY_PREFIX, email);
    use_store()
        .set(
            key.as_str(),
            &request,
            Some(CHANGE_PASSWORD_REQUEST_TTL_SECS),
        )
        .await
}

pub async fn delete_change_password_request(email: &str) -> Result<bool, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, email);
    store.delete(key.as_str()).await
}
