use crate::data::store::use_store;
use crate::iam::oidc::continuation::LoginContinuation;
use store::Store;

use super::signup::{PendingSignupProfile, SignupOrigin};

const REQUEST_TTL_SECS: u64 = 60 * 60; // 60 minutes

pub async fn create_email_verification_request(
    profile: &PendingSignupProfile,
) -> Result<String, store::StoreError> {
    let store = use_store();
    let sid = ulid::Ulid::generate().to_string();

    let key1 = store::keys::account::email_verification(&sid);
    let key2 = store::keys::account::email_verification_lookup(&profile.email);
    store
        .set(key1.as_str(), profile, Some(REQUEST_TTL_SECS))
        .await?;
    store
        .set(key2.as_str(), &sid, Some(REQUEST_TTL_SECS))
        .await?;
    Ok(sid)
}

/// Point a pending request at the app a newly requested link was started
/// from, so the link works in another tab too. An invitation keeps the app the
/// administrator chose.
pub async fn set_continuation(
    sid: &str,
    continuation: LoginContinuation,
) -> Result<(), store::StoreError> {
    let Some(mut profile) = get_email_verification_request(sid).await? else {
        return Ok(());
    };
    if profile.origin != SignupOrigin::SelfService
        || profile.continuation.as_ref() == Some(&continuation)
    {
        return Ok(());
    }
    profile.continuation = Some(continuation);
    let key = store::keys::account::email_verification(sid);
    use_store()
        .set(key.as_str(), &profile, Some(REQUEST_TTL_SECS))
        .await
}

pub async fn check_email_verification_request(
    email: &str,
) -> Result<Option<String>, store::StoreError> {
    let store = use_store();
    let key = store::keys::account::email_verification_lookup(email);
    store.get::<String>(key.as_str()).await
}

pub async fn get_email_verification_request(
    sid: &str,
) -> Result<Option<PendingSignupProfile>, store::StoreError> {
    let store = use_store();
    let key = store::keys::account::email_verification(sid);
    store.get::<PendingSignupProfile>(key.as_str()).await
}

pub async fn delete_email_verification_request(sid: &str) -> Result<(), store::StoreError> {
    let store = use_store();
    if let Some(profile) = get_email_verification_request(sid).await? {
        let key1 = store::keys::account::email_verification(sid);
        let key2 = store::keys::account::email_verification_lookup(&profile.email);
        store.delete(key1.as_str()).await?;
        store.delete(key2.as_str()).await?;
    }
    Ok(())
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::{
        create_email_verification_request, delete_email_verification_request,
        get_email_verification_request, set_continuation,
    };
    use crate::iam::oidc::continuation::LoginContinuation;
    use crate::iam::user::signup::{PendingSignupProfile, SignupOrigin};
    use tools::Locale;

    #[tokio::test]
    async fn a_self_service_request_takes_the_app_of_the_new_link() {
        let profile = PendingSignupProfile::self_service("self@example.com", Locale::En);
        let sid = create_email_verification_request(&profile).await.unwrap();

        set_continuation(&sid, LoginContinuation::new("orders", None))
            .await
            .unwrap();

        let pending = get_email_verification_request(&sid).await.unwrap().unwrap();
        assert_eq!(
            pending.continuation,
            Some(LoginContinuation::new("orders", None))
        );
        delete_email_verification_request(&sid).await.unwrap();
    }

    #[tokio::test]
    async fn an_invitation_keeps_the_app_the_administrator_chose() {
        let profile = PendingSignupProfile {
            origin: SignupOrigin::Invitation,
            continuation: Some(LoginContinuation::new("orders", None)),
            ..PendingSignupProfile::self_service("invited@example.com", Locale::En)
        };
        let sid = create_email_verification_request(&profile).await.unwrap();

        set_continuation(&sid, LoginContinuation::new("elsewhere", None))
            .await
            .unwrap();

        let pending = get_email_verification_request(&sid).await.unwrap().unwrap();
        assert_eq!(
            pending.continuation,
            Some(LoginContinuation::new("orders", None))
        );
        delete_email_verification_request(&sid).await.unwrap();
    }
}
