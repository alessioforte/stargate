use crate::data::store::use_store;
use crate::iam::oidc::continuation::LoginContinuation;
use serde::{Deserialize, Serialize};
use store::Store;

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

impl ChangePasswordRequest {
    fn new() -> Self {
        Self {
            sid: ulid::Ulid::generate().to_string(),
            continuation: None,
        }
    }
}

/// The pending reset of an address: created, or renewed so the link just
/// issued lives exactly as long as the request behind it. The app of the
/// newest link replaces the previous one.
pub async fn issue_change_password_request(
    email: &str,
    continuation: Option<LoginContinuation>,
) -> Result<String, store::StoreError> {
    let mut request = match get_change_password_request(email).await? {
        Some(request) => request,
        None => ChangePasswordRequest::new(),
    };
    if let Some(continuation) = continuation {
        request.continuation = Some(continuation);
    }
    let key = store::keys::account::password_reset(email);
    use_store()
        .set(
            key.as_str(),
            &request,
            Some(CHANGE_PASSWORD_REQUEST_TTL_SECS),
        )
        .await?;
    Ok(request.sid)
}

pub async fn get_change_password_request(
    email: &str,
) -> Result<Option<ChangePasswordRequest>, store::StoreError> {
    let store = use_store();
    let key = store::keys::account::password_reset(email);
    store.get::<ChangePasswordRequest>(key.as_str()).await
}

pub async fn delete_change_password_request(email: &str) -> Result<bool, store::StoreError> {
    let store = use_store();
    let key = store::keys::account::password_reset(email);
    store.delete(key.as_str()).await
}
