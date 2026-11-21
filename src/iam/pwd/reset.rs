use crate::data::store::use_store;
use crate::iam::oidc::continuation::LoginContinuation;
use serde::{Deserialize, Serialize};
use store::Store;

/// Lifetime of a change-password request (and the reset tokens minted for it).
pub const CHANGE_PASSWORD_REQUEST_TTL_SECS: u64 = 60 * 60;

/// How many times a write races a concurrent change before it gives up.
const ISSUE_ATTEMPTS: usize = 3;

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

    fn with_continuation(mut self, continuation: Option<LoginContinuation>) -> Self {
        if let Some(continuation) = continuation {
            self.continuation = Some(continuation);
        }
        self
    }
}

/// The pending reset of an address: created, or renewed so the link just
/// issued lives exactly as long as the request behind it. The app of the
/// newest link replaces the previous one.
///
/// A renewal only writes the request it read: a reset consumed in the
/// meantime must never be brought back, because its token would allow a
/// second password change after the first one succeeded.
pub async fn issue_change_password_request(
    email: &str,
    continuation: Option<LoginContinuation>,
) -> Result<String, store::StoreError> {
    for _ in 0..ISSUE_ATTEMPTS {
        let observed = get_change_password_request(email).await?;
        let request = match &observed {
            Some(current) => current.clone().with_continuation(continuation.clone()),
            None => ChangePasswordRequest::new().with_continuation(continuation.clone()),
        };
        if write_change_password_request(email, observed.as_ref(), &request).await? {
            return Ok(request.sid);
        }
    }
    Err(store::StoreError::Conflict(
        "change-password request kept changing during its renewal".to_string(),
    ))
}

/// Write the issued request only when the store still holds `observed`, or
/// nothing when it is `None`, and renew its deadline. `false` means another
/// writer got there first.
async fn write_change_password_request(
    email: &str,
    observed: Option<&ChangePasswordRequest>,
    request: &ChangePasswordRequest,
) -> Result<bool, store::StoreError> {
    let key = store::keys::account::password_reset(email);
    let ttl = Some(CHANGE_PASSWORD_REQUEST_TTL_SECS);
    let store = use_store();
    match observed {
        Some(observed) => {
            store
                .compare_and_swap(key.as_str(), observed, request, ttl)
                .await
        }
        None => store.set_if_absent(key.as_str(), request, ttl).await,
    }
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

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::{
        delete_change_password_request, get_change_password_request, issue_change_password_request,
        write_change_password_request,
    };
    use crate::iam::oidc::continuation::LoginContinuation;

    #[tokio::test]
    async fn a_request_consumed_between_read_and_write_is_not_written_back() {
        let email = "consumed@example.com";
        let sid = issue_change_password_request(email, None).await.unwrap();
        let observed = get_change_password_request(email).await.unwrap().unwrap();
        assert_eq!(observed.sid, sid);

        // `PUT /account/credentials` consumes the request before changing the
        // password; the renewal's write must then fail instead of resurrecting
        // the sid its link still carries.
        assert!(delete_change_password_request(email).await.unwrap());
        let next = observed
            .clone()
            .with_continuation(Some(LoginContinuation::new("orders", None)));
        assert!(
            !write_change_password_request(email, Some(&observed), &next)
                .await
                .unwrap()
        );
        assert!(get_change_password_request(email).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn issuing_reuses_a_live_request_and_creates_a_new_one_after_consumption() {
        let email = "issued@example.com";
        let first = issue_change_password_request(email, None).await.unwrap();
        let renewed =
            issue_change_password_request(email, Some(LoginContinuation::new("orders", None)))
                .await
                .unwrap();
        assert_eq!(renewed, first);
        assert_eq!(
            get_change_password_request(email)
                .await
                .unwrap()
                .unwrap()
                .continuation,
            Some(LoginContinuation::new("orders", None))
        );

        assert!(delete_change_password_request(email).await.unwrap());
        let replacement = issue_change_password_request(email, None).await.unwrap();
        assert_ne!(replacement, first);
        let _ = delete_change_password_request(email).await;
    }
}
