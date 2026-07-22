use crate::etc::{self, ext::RequestExt, sub::Subject};
use lru::LruCache;
use std::cell::RefCell;
use std::fmt;
use std::num::NonZeroUsize;
use store::Store;
use tracing::error;

const HASH_CACHE_MAX_SIZE: usize = 256;

thread_local! {
    static HASH_CACHE: RefCell<LruCache<Box<str>, Box<str>>> =
        RefCell::new(LruCache::new(NonZeroUsize::new(HASH_CACHE_MAX_SIZE).unwrap()));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    ApiKey,
    Jwt,
    Anonymous,
}

#[derive(Clone)]
enum VerifiedAuthentication {
    ApiKey,
    Jwt {
        session_id: Box<str>,
        auth_time: Option<i64>,
    },
    Anonymous,
}

/// Effective gateway identity produced only after credential and backing-state
/// verification. Subject attributes remain available to existing policies but
/// are deliberately not exposed as propagation facts.
#[derive(Clone)]
pub struct VerifiedIdentity {
    subject: Option<Subject>,
    authentication: VerifiedAuthentication,
}

impl VerifiedIdentity {
    pub fn anonymous() -> Self {
        Self {
            subject: None,
            authentication: VerifiedAuthentication::Anonymous,
        }
    }

    fn api_key(subject: Subject) -> Option<Self> {
        (subject.sub_type == crate::etc::sub::SubjectType::ApiKey).then_some(Self {
            subject: Some(subject),
            authentication: VerifiedAuthentication::ApiKey,
        })
    }

    fn jwt(subject: Subject, session_id: String, auth_time: Option<usize>) -> Option<Self> {
        if subject.sub_type != crate::etc::sub::SubjectType::User || session_id.trim().is_empty() {
            return None;
        }
        let auth_time = auth_time.and_then(|value| i64::try_from(value).ok());
        Some(Self {
            subject: Some(subject),
            authentication: VerifiedAuthentication::Jwt {
                session_id: session_id.into_boxed_str(),
                auth_time,
            },
        })
    }

    pub fn auth_kind(&self) -> AuthKind {
        match self.authentication {
            VerifiedAuthentication::ApiKey => AuthKind::ApiKey,
            VerifiedAuthentication::Jwt { .. } => AuthKind::Jwt,
            VerifiedAuthentication::Anonymous => AuthKind::Anonymous,
        }
    }

    pub fn subject(&self) -> Option<&Subject> {
        self.subject.as_ref()
    }

    pub fn session_id(&self) -> Option<&str> {
        match &self.authentication {
            VerifiedAuthentication::Jwt { session_id, .. } => Some(session_id),
            VerifiedAuthentication::ApiKey | VerifiedAuthentication::Anonymous => None,
        }
    }

    pub fn auth_time(&self) -> Option<i64> {
        match self.authentication {
            VerifiedAuthentication::Jwt { auth_time, .. } => auth_time,
            VerifiedAuthentication::ApiKey | VerifiedAuthentication::Anonymous => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_jwt(subject: Subject, session_id: String, auth_time: Option<usize>) -> Self {
        Self::jwt(subject, session_id, auth_time).expect("valid test JWT identity")
    }

    #[cfg(test)]
    pub(crate) fn test_api_key(subject: Subject) -> Self {
        Self::api_key(subject).expect("valid test API-key identity")
    }
}

impl fmt::Debug for VerifiedIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedIdentity")
            .field("auth_kind", &self.auth_kind())
            .field("has_subject", &self.subject.is_some())
            .finish_non_exhaustive()
    }
}

fn cached_hash_api_key(api_key: &str) -> String {
    HASH_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(hash) = cache.get(api_key) {
            return hash.to_string();
        }
        let hash = pw::hash_api_key(api_key);
        cache.put(Box::from(api_key), Box::from(hash.as_str()));
        hash
    })
}

/// Verify an API key and return its persisted or cached effective identity.
pub async fn verify_api_key<R: RequestExt + ?Sized>(req: &R) -> Option<VerifiedIdentity> {
    let api_key = req.get_api_key()?;
    let hash_key = cached_hash_api_key(&api_key);
    let store = etc::store::use_store();
    let session = store.get::<Subject>(&hash_key).await.unwrap_or(None);

    if let Some(subject) = session {
        return VerifiedIdentity::api_key(subject);
    }

    let result = match crate::db::get_api_key_auth_by_hash(&hash_key).await {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to get api key: {}", e);
            return None;
        }
    };

    if let Some(auth) = result {
        if auth.api_key.revoked {
            return None;
        }

        let subject = Subject::from(auth);
        let ttl = Some(3600);
        match store.set(&hash_key, &subject, ttl).await {
            Ok(_) => (),
            Err(e) => {
                error!("Failed to store subject in the session store: {}", e);
            }
        }

        return VerifiedIdentity::api_key(subject);
    }

    None
}

/// Remove cached auth subjects for the given API key hashes. Call after
/// revoking keys so they stop authenticating immediately instead of when
/// the cache entry expires. Best-effort: failures are logged and the cache
/// entry dies at its TTL.
pub async fn purge_api_key_subjects(key_hashes: &[String]) {
    let store = etc::store::use_store();
    for hash in key_hashes {
        if let Err(e) = store.delete(hash).await {
            error!("Failed to purge cached api key subject: {}", e);
        }
    }
}

/// Verify a session JWT and bind its validated session metadata to the current
/// subject loaded from trusted state.
pub async fn verify_jwt<R: RequestExt + ?Sized>(req: &R) -> Option<VerifiedIdentity> {
    let token = req.get_token()?;

    let jwt = etc::jwt::jwt_config();
    let claims = match jwt.validate_session_access_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return None;
        }
    };

    match crate::act::token_revocation::is_revoked(&claims).await {
        Ok(false) => {}
        Ok(true) => return None,
        Err(e) => {
            error!("Failed to check token revocation state: {}", e);
            return None;
        }
    }

    let store = etc::store::use_store();
    let sid = claims.sid.clone()?;
    match store.get::<Subject>(&sid).await {
        Ok(Some(subject)) => VerifiedIdentity::jwt(subject, sid, claims.auth_time),
        Ok(None) => None,
        Err(e) => {
            error!("Failed to get subject from the session store: {}", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etc::sub::{Subject, SubjectType};
    use serde_json::json;

    #[test]
    fn verified_jwt_identity_keeps_only_validated_session_metadata() {
        let mut subject = Subject::new(
            "01JZ000000000000000000000A".to_owned(),
            SubjectType::User,
            Some(json!({ "secret": "must-not-propagate" })),
        );
        subject.org_id = Some("01JZ000000000000000000000Z".to_owned());
        subject.org_role = Some("admin".to_owned());

        let identity = VerifiedIdentity::jwt(
            subject,
            "01JZ000000000000000000000B".to_owned(),
            Some(1_784_473_000),
        )
        .unwrap();

        assert_eq!(identity.auth_kind(), AuthKind::Jwt);
        assert_eq!(identity.session_id(), Some("01JZ000000000000000000000B"));
        assert_eq!(identity.auth_time(), Some(1_784_473_000));
        assert_eq!(
            identity.subject().unwrap().attrs["secret"],
            "must-not-propagate"
        );
        assert!(!format!("{identity:?}").contains("must-not-propagate"));
    }

    #[test]
    fn verified_api_key_identity_has_no_session_metadata() {
        let subject = Subject::new(
            "01JZ000000000000000000000K".to_owned(),
            SubjectType::ApiKey,
            None,
        );
        let identity = VerifiedIdentity::api_key(subject).unwrap();

        assert_eq!(identity.auth_kind(), AuthKind::ApiKey);
        assert_eq!(identity.session_id(), None);
        assert_eq!(identity.auth_time(), None);
    }

    #[test]
    fn anonymous_identity_has_no_subject_or_session() {
        let identity = VerifiedIdentity::anonymous();

        assert_eq!(identity.auth_kind(), AuthKind::Anonymous);
        assert!(identity.subject().is_none());
        assert_eq!(identity.session_id(), None);
        assert_eq!(identity.auth_time(), None);
    }

    #[test]
    fn authentication_kind_must_match_subject_type() {
        let user = Subject::new("user".to_owned(), SubjectType::User, None);
        let api_key = Subject::new("key".to_owned(), SubjectType::ApiKey, None);

        assert!(VerifiedIdentity::api_key(user).is_none());
        assert!(VerifiedIdentity::jwt(api_key, "sid".to_owned(), None).is_none());
    }
}
