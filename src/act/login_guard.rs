use crate::etc::store::use_store;
use store::{AtomicStore, Store};

const KEY_PREFIX: &str = "login_attempts:v2";
const MAX_SOURCE_ATTEMPTS: u32 = 20;
const MAX_SUBJECT_ATTEMPTS: u32 = 5;
const LOCKOUT_SECS: u64 = 300; // 5 minutes

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginThrottle {
    source_key: String,
    subject_key: String,
}

impl LoginThrottle {
    pub fn for_user(client_ip: &str, user_id: &str) -> Self {
        Self {
            source_key: source_key(client_ip),
            subject_key: user_key(client_ip, user_id),
        }
    }

    pub fn for_identifier(client_ip: &str, identifier: &str) -> Self {
        Self {
            source_key: source_key(client_ip),
            subject_key: identifier_key(client_ip, identifier),
        }
    }
}

pub async fn check_login_allowed(throttle: &LoginThrottle) -> Result<bool, store::StoreError> {
    let source_attempts = read_attempts(&throttle.source_key).await?;
    if source_attempts >= MAX_SOURCE_ATTEMPTS {
        return Ok(false);
    }

    let subject_attempts = read_attempts(&throttle.subject_key).await?;
    Ok(subject_attempts < MAX_SUBJECT_ATTEMPTS)
}

pub async fn record_failed_attempt(throttle: &LoginThrottle) -> Result<(), store::StoreError> {
    let store = use_store();
    let ttl = Some(LOCKOUT_SECS);

    let _ = store.incr_i64(&throttle.source_key, 1, ttl).await?;
    let _ = store.incr_i64(&throttle.subject_key, 1, ttl).await?;

    Ok(())
}

pub async fn clear_subject_attempts(throttle: &LoginThrottle) -> Result<(), store::StoreError> {
    let store = use_store();
    // Keep the broader per-IP bucket so successful logins do not erase source-wide spray signals.
    let _ = store.delete(&throttle.subject_key).await?;
    Ok(())
}

pub fn lockout_seconds() -> u64 {
    LOCKOUT_SECS
}

fn source_key(client_ip: &str) -> String {
    format!("{KEY_PREFIX}:ip:{client_ip}")
}

fn user_key(client_ip: &str, user_id: &str) -> String {
    format!("{KEY_PREFIX}:user:{user_id}:ip:{client_ip}")
}

fn identifier_key(client_ip: &str, identifier: &str) -> String {
    format!("{KEY_PREFIX}:identifier:{identifier}:ip:{client_ip}")
}

async fn read_attempts(key: &str) -> Result<u32, store::StoreError> {
    let store = use_store();
    let attempts = store.get_i64(key).await?.unwrap_or(0);
    Ok(saturating_attempts(attempts))
}

fn saturating_attempts(value: i64) -> u32 {
    if value <= 0 {
        return 0;
    }

    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::LoginThrottle;

    #[test]
    fn login_throttle_for_user_uses_canonical_user_bucket() {
        let throttle = LoginThrottle::for_user("127.0.0.1", "user_123");

        assert_eq!(throttle.source_key, "login_attempts:v2:ip:127.0.0.1");
        assert_eq!(
            throttle.subject_key,
            "login_attempts:v2:user:user_123:ip:127.0.0.1"
        );
    }

    #[test]
    fn login_throttle_for_identifier_uses_identifier_bucket() {
        let throttle = LoginThrottle::for_identifier("127.0.0.1", "alice@example.com");

        assert_eq!(throttle.source_key, "login_attempts:v2:ip:127.0.0.1");
        assert_eq!(
            throttle.subject_key,
            "login_attempts:v2:identifier:alice@example.com:ip:127.0.0.1"
        );
    }
}
