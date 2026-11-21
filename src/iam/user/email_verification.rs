use crate::data::store::use_store;
use crate::iam::oidc::continuation::LoginContinuation;
use store::Store;

use super::signup::{self, PENDING_SIGNUP_TTL_SECS, PendingSignupProfile, SignupOrigin};

pub async fn create_email_verification_request(
    profile: &PendingSignupProfile,
) -> Result<String, store::StoreError> {
    let sid = ulid::Ulid::generate().to_string();
    store_email_verification_request(&sid, profile).await?;
    Ok(sid)
}

/// Point the pending request at the app its newest link was started from and
/// renew it, so every key of one request expires together and the link just
/// sent lives as long as the request behind it. An invitation keeps the app
/// the administrator chose.
pub async fn renew_email_verification_request(
    sid: &str,
    continuation: Option<LoginContinuation>,
) -> Result<(), store::StoreError> {
    let Some(mut profile) = get_email_verification_request(sid).await? else {
        return Ok(());
    };
    if profile.origin == SignupOrigin::SelfService
        && let Some(continuation) = continuation
    {
        profile.continuation = Some(continuation);
    }
    store_email_verification_request(sid, &profile).await?;

    // The copy created when the link is opened carries the continuation the
    // completion page reads: keep it with the request, renewal included.
    if signup::get_signup_request(sid).await?.is_some() {
        signup::store_signup_request(sid, &profile).await?;
    }
    Ok(())
}

/// Both keys of the request share one write and one deadline.
async fn store_email_verification_request(
    sid: &str,
    profile: &PendingSignupProfile,
) -> Result<(), store::StoreError> {
    let store = use_store();
    let ttl = Some(PENDING_SIGNUP_TTL_SECS);
    store
        .set(
            store::keys::account::email_verification(sid).as_str(),
            profile,
            ttl,
        )
        .await?;
    store
        .set(
            store::keys::account::email_verification_lookup(&profile.email).as_str(),
            &sid,
            ttl,
        )
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
        get_email_verification_request, renew_email_verification_request,
    };
    use crate::data::store::use_store;
    use crate::iam::oidc::continuation::LoginContinuation;
    use crate::iam::user::signup::{self, PendingSignupProfile, SignupOrigin};
    use store::keys;
    use tools::Locale;

    /// A renewal starts from keys created a while ago, so a key left at its
    /// original deadline is visible.
    async fn wait_for_the_clock() {
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    }

    fn within_one_second(
        first: chrono::DateTime<chrono::Utc>,
        second: chrono::DateTime<chrono::Utc>,
    ) -> bool {
        (first - second).num_milliseconds().abs() < 1000
    }

    #[tokio::test]
    async fn renewing_a_request_keeps_its_keys_on_one_deadline() {
        let profile = PendingSignupProfile::self_service("self@example.com", Locale::En);
        let sid = create_email_verification_request(&profile).await.unwrap();
        // The user opens the link: `GET /signup` stores the completion copy.
        signup::store_signup_request(&sid, &profile).await.unwrap();
        let store = use_store();
        let before = store
            .expiry(&keys::account::email_verification(&sid))
            .expect("profile deadline");
        wait_for_the_clock().await;

        renew_email_verification_request(&sid, Some(LoginContinuation::new("orders", None)))
            .await
            .unwrap();

        let profile_deadline = store
            .expiry(&keys::account::email_verification(&sid))
            .expect("profile deadline");
        let lookup_deadline = store
            .expiry(&keys::account::email_verification_lookup(&profile.email))
            .expect("lookup deadline");
        let copy_deadline = store
            .expiry(&keys::account::signup(&sid))
            .expect("copy deadline");

        assert!(
            profile_deadline > before + chrono::Duration::seconds(1),
            "the profile deadline is renewed"
        );
        assert!(
            within_one_second(profile_deadline, lookup_deadline),
            "{profile_deadline} and {lookup_deadline} differ"
        );
        assert!(
            within_one_second(profile_deadline, copy_deadline),
            "{profile_deadline} and {copy_deadline} differ"
        );
        assert_eq!(
            super::check_email_verification_request(&profile.email)
                .await
                .unwrap(),
            Some(sid.clone())
        );
        assert_eq!(
            get_email_verification_request(&sid)
                .await
                .unwrap()
                .unwrap()
                .continuation,
            Some(LoginContinuation::new("orders", None))
        );
        assert_eq!(
            signup::get_signup_request(&sid)
                .await
                .unwrap()
                .unwrap()
                .continuation,
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
        signup::store_signup_request(&sid, &profile).await.unwrap();

        renew_email_verification_request(&sid, Some(LoginContinuation::new("elsewhere", None)))
            .await
            .unwrap();

        let pending = get_email_verification_request(&sid).await.unwrap().unwrap();
        assert_eq!(
            pending.continuation,
            Some(LoginContinuation::new("orders", None))
        );
        assert_eq!(
            signup::get_signup_request(&sid)
                .await
                .unwrap()
                .unwrap()
                .continuation,
            Some(LoginContinuation::new("orders", None))
        );
        delete_email_verification_request(&sid).await.unwrap();
    }
}
