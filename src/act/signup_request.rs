use crate::etc::store::use_store;
use db::ent::NewOrganizationMembership;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use store::Store;

const KEY_PREFIX: &str = "signup_requests";

/// How a pending signup started, which decides whether `SIGNUP_MODE` still
/// lets it complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignupOrigin {
    /// `POST /signup`: completes only while `SIGNUP_MODE=open`.
    SelfService,
    /// An admin invitation: completes unless `SIGNUP_MODE=closed`.
    Invitation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSignupProfile {
    pub origin: SignupOrigin,
    pub email: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    pub attrs: Value,
    pub membership: Option<NewOrganizationMembership>,
}

impl PendingSignupProfile {
    /// A self-registration: only the address is known.
    pub fn self_service(email: impl Into<String>) -> Self {
        Self {
            origin: SignupOrigin::SelfService,
            email: email.into(),
            given_name: None,
            family_name: None,
            nickname: None,
            picture: None,
            phone_number: None,
            attrs: Value::Object(serde_json::Map::new()),
            membership: None,
        }
    }
}

pub async fn create_signup_request(
    sid: &str,
    profile: &PendingSignupProfile,
) -> Result<(), store::StoreError> {
    let store = use_store();

    let key = format!("{}:{}", KEY_PREFIX, sid);
    let ttl = Some(60 * 60); // 60 minutes
    store.set(key.as_str(), profile, ttl).await
}

pub async fn get_signup_request(
    sid: &str,
) -> Result<Option<PendingSignupProfile>, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, sid);
    store.get::<PendingSignupProfile>(key.as_str()).await
}

pub async fn delete_signup_request(sid: &str) -> Result<bool, store::StoreError> {
    let store = use_store();
    let key = format!("{}:{}", KEY_PREFIX, sid);
    store.delete(key.as_str()).await
}

#[cfg(test)]
mod tests {
    use super::{PendingSignupProfile, SignupOrigin};

    #[test]
    fn pending_profile_roundtrips_server_owned_membership() {
        let profile: PendingSignupProfile = serde_json::from_value(serde_json::json!({
            "origin": "invitation",
            "email": "member@example.com",
            "givenName": null,
            "familyName": null,
            "nickname": null,
            "picture": null,
            "phoneNumber": null,
            "attrs": {},
            "membership": {
                "organizationId": "01JZ0000000000000000000001",
                "role": "admin"
            }
        }))
        .expect("deserialize pending membership");

        assert_eq!(profile.origin, SignupOrigin::Invitation);
        let membership = profile.membership.expect("membership");
        assert_eq!(membership.organization_id, "01JZ0000000000000000000001");
        assert_eq!(membership.role, "admin");
    }
}
