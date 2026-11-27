//! Password attempt throttles. Every bucket that names a source counts the
//! client network (`srv::client_ip::ClientNetwork`): the IPv4 address or the
//! IPv6 block of the listener's prefix, so rotating addresses inside one
//! subscriber's /64 never opens fresh buckets. Recognition after a completed
//! login follows the same unit: it lifts only the account-wide pause, while the
//! network and account/network limits keep counting every attempt.

use crate::data::store::use_store;
use crate::web::request::UNKNOWN_CLIENT;
use serde::{Deserialize, Serialize};
use store::{Store, StoreError, StoreResult};

const MAX_IP_ATTEMPTS: u32 = 20;
const MAX_SUBJECT_IP_ATTEMPTS: u32 = 5;
const BACKOFF_THRESHOLD: u32 = 5;
const WINDOW_MS: i64 = 300_000;
const MAX_BACKOFF_SECS: u64 = 30;
const MAX_CAS_ATTEMPTS: usize = 32;
// A month covers routine returning users while bounding recognition of an
// address that may later be reassigned. Only a completed login renews it.
const KNOWN_IP_TTL_SECS: u64 = 30 * 24 * 60 * 60;

struct Bucket {
    key: String,
    policy: Policy,
}

struct KnownAddress {
    key: String,
    field: String,
}

#[derive(Clone, Copy)]
enum Policy {
    FixedWindow { limit: u32 },
    AccountBackoff,
}

pub(crate) struct LoginThrottle {
    buckets: [Bucket; 3],
    known: Option<KnownAddress>,
}

pub(crate) enum Admission<T> {
    Allowed(T),
    Limited { retry_after: u64 },
}

/// A reservation holds capacity before password verification begins. A wrong
/// password leaves it charged; only refund a reservation that did not fail.
#[must_use]
pub(crate) struct LoginAttempt<'a>(Vec<Reservation<'a>>);

impl LoginThrottle {
    pub(crate) fn for_user(client: &str, user_id: &str) -> Self {
        let mut throttle = Self::new(client, |ip| store::keys::account::login_user(user_id, ip));
        throttle.known = Some(KnownAddress {
            key: store::keys::account::login_known(user_id),
            field: client.to_string(),
        });
        throttle
    }

    pub(crate) fn for_identifier(client: &str, identifier: &str) -> Self {
        Self::new(client, |ip| {
            store::keys::account::login_identifier(identifier, ip)
        })
    }

    fn new(client: &str, subject_key: impl Fn(Option<&str>) -> String) -> Self {
        Self {
            known: None,
            buckets: [
                Bucket {
                    key: store::keys::account::login_ip(client),
                    policy: Policy::FixedWindow {
                        limit: MAX_IP_ATTEMPTS,
                    },
                },
                Bucket {
                    key: subject_key(Some(client)),
                    policy: Policy::FixedWindow {
                        limit: MAX_SUBJECT_IP_ATTEMPTS,
                    },
                },
                Bucket {
                    key: subject_key(None),
                    policy: Policy::AccountBackoff,
                },
            ],
        }
    }

    pub(crate) async fn reserve(&self) -> StoreResult<Admission<LoginAttempt<'_>>> {
        let store = use_store();
        let known_ip = match &self.known {
            Some(known) => store
                .hget::<i64>(&known.key, &known.field)
                .await?
                .is_some_and(|expires_at| expires_at > chrono::Utc::now().timestamp()),
            None => false,
        };
        let buckets = if known_ip {
            &self.buckets[..2]
        } else {
            &self.buckets
        };
        let mut attempt = LoginAttempt(Vec::with_capacity(buckets.len()));
        // IP and account have different Redis slots. Each bucket enforces its
        // own limit atomically. Only verify once every reservation succeeds.
        for bucket in buckets {
            match bucket.reserve(store).await {
                Ok(Admission::Allowed(reservation)) => attempt.0.push(reservation),
                Ok(Admission::Limited { retry_after }) => {
                    attempt.refund().await;
                    return Ok(Admission::Limited { retry_after });
                }
                Err(error) => {
                    attempt.refund().await;
                    return Err(error);
                }
            }
        }
        Ok(Admission::Allowed(attempt))
    }
}

/// Called only after the password flow has issued a session, including MFA
/// when required. A failed write leaves the ordinary account backoff in force.
pub(crate) async fn record_success(user_id: &str, client: &str) {
    if client == UNKNOWN_CLIENT {
        return;
    }
    let key = store::keys::account::login_known(user_id);
    let expires_at = chrono::Utc::now().timestamp() + KNOWN_IP_TTL_SECS as i64;
    if let Err(error) = use_store()
        .hset(&key, client, &expires_at, Some(KNOWN_IP_TTL_SECS))
        .await
    {
        tracing::warn!(%error, "failed to remember password login address");
    }
}

/// Seeds the account-wide bucket at its longest pause so a test does not wait
/// out the progressive delay.
#[cfg(test)]
pub(crate) async fn seed_account_backoff(user_id: &str) {
    let paused = Attempts {
        count: 40,
        ..Attempts::new(chrono::Utc::now().timestamp_millis())
    };
    use_store()
        .set(
            &store::keys::account::login_user(user_id, None),
            &paused,
            Some(300),
        )
        .await
        .unwrap();
}

impl LoginAttempt<'_> {
    pub(crate) async fn refund(self) {
        for reservation in self.0.into_iter().rev() {
            if let Err(error) = reservation.refund(use_store()).await {
                tracing::warn!(%error, "login attempt refund failed");
            }
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Attempts {
    generation: [u8; 16],
    count: u32,
    last_reserved_at: i64,
    expires_at: i64,
}

impl Attempts {
    fn new(now: i64) -> Self {
        Self {
            generation: ulid::Ulid::generate().to_bytes(),
            count: 0,
            last_reserved_at: now,
            expires_at: now + WINDOW_MS,
        }
    }

    fn ttl(self, now: i64) -> Option<u64> {
        Some(self.expires_at.abs_diff(now).div_ceil(1000))
    }
}

impl Policy {
    fn decide(self, attempts: Attempts, now: i64) -> Admission<Attempts> {
        let blocked_until = match self {
            Self::FixedWindow { limit } if attempts.count >= limit => attempts.expires_at,
            Self::AccountBackoff if attempts.count >= BACKOFF_THRESHOLD => {
                let exponent = (attempts.count - BACKOFF_THRESHOLD).min(4);
                let delay = (2_u64 << exponent).min(MAX_BACKOFF_SECS);
                attempts.last_reserved_at + (delay * 1000) as i64
            }
            _ => now,
        };
        if blocked_until > now {
            return Admission::Limited {
                retry_after: blocked_until.abs_diff(now).div_ceil(1000),
            };
        }

        let last_reserved_at = attempts.last_reserved_at.max(now);
        let expires_at = match self {
            Self::FixedWindow { .. } => attempts.expires_at,
            Self::AccountBackoff => last_reserved_at + WINDOW_MS,
        };
        Admission::Allowed(Attempts {
            count: attempts.count.saturating_add(1),
            last_reserved_at,
            expires_at,
            ..attempts
        })
    }
}

struct Reservation<'a> {
    key: &'a str,
    generation: [u8; 16],
}

impl Bucket {
    async fn reserve(&self, store: &impl Store) -> StoreResult<Admission<Reservation<'_>>> {
        for _ in 0..MAX_CAS_ATTEMPTS {
            let current = store.get::<Attempts>(&self.key).await?;
            let now = chrono::Utc::now().timestamp_millis();
            let attempts = current
                .filter(|attempts| attempts.expires_at > now)
                .unwrap_or_else(|| Attempts::new(now));
            let next = match self.policy.decide(attempts, now) {
                Admission::Allowed(next) => next,
                Admission::Limited { retry_after } => {
                    return Ok(Admission::Limited { retry_after });
                }
            };
            let reserved = match current {
                Some(current) => {
                    store
                        .compare_and_swap(&self.key, &current, &next, next.ttl(now))
                        .await?
                }
                None => store.set_if_absent(&self.key, &next, next.ttl(now)).await?,
            };
            if reserved {
                return Ok(Admission::Allowed(Reservation {
                    key: &self.key,
                    generation: next.generation,
                }));
            }
        }
        Err(StoreError::Timeout(
            "login attempt reservation contention".into(),
        ))
    }
}

impl Reservation<'_> {
    async fn refund(self, store: &impl Store) -> StoreResult<()> {
        for _ in 0..MAX_CAS_ATTEMPTS {
            let Some(current) = store.get::<Attempts>(self.key).await? else {
                return Ok(());
            };
            let now = chrono::Utc::now().timestamp_millis();
            // A delayed refund must never credit a replacement window.
            if current.generation != self.generation || current.expires_at <= now {
                return Ok(());
            }
            let next = Attempts {
                count: current.count.saturating_sub(1),
                ..current
            };
            if store
                .compare_and_swap(self.key, &current, &next, next.ttl(now))
                .await?
            {
                return Ok(());
            }
        }
        Err(StoreError::Timeout(
            "login attempt refund contention".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "memory")]
    #[tokio::test]
    async fn refund_cannot_credit_a_replacement_window() {
        let store = store::MemoryStore::new();
        let bucket = Bucket {
            key: "login-test".into(),
            policy: Policy::FixedWindow { limit: 1 },
        };
        let old = Attempts {
            count: 1,
            ..Attempts::new(0)
        };
        // Simulate an expired logical window still present in the store.
        store.set(&bucket.key, &old, Some(60)).await.unwrap();
        let stale = Reservation {
            key: &bucket.key,
            generation: old.generation,
        };

        assert!(matches!(
            bucket.reserve(&store).await.unwrap(),
            Admission::Allowed(_)
        ));
        stale.refund(&store).await.unwrap();
        assert!(matches!(
            bucket.reserve(&store).await.unwrap(),
            Admission::Limited { .. }
        ));
    }

    #[tokio::test]
    #[cfg_attr(feature = "redis", ignore = "requires the disposable Redis fixture")]
    async fn known_addresses_bypass_only_account_backoff() {
        #[cfg(feature = "redis")]
        crate::data::store::init().await.unwrap();

        let store = use_store();
        let user_id = ulid::Ulid::generate().to_string();
        let known_ip = "2001:db8:aaaa::/64";
        let throttle = LoginThrottle::for_user(known_ip, &user_id);
        // Seed the account at maximum backoff so the assertions have a stable
        // 30-second pause, independently of Argon2 speed or scheduler load.
        seed_account_backoff(&user_id).await;
        assert!(matches!(
            throttle.reserve().await.unwrap(),
            Admission::Limited { .. }
        ));
        let now = chrono::Utc::now().timestamp();

        record_success(&user_id, known_ip).await;
        let known = throttle.known.as_ref().unwrap();
        let expires_at = store
            .hget::<i64>(&known.key, &known.field)
            .await
            .unwrap()
            .unwrap();
        assert!(
            (now + KNOWN_IP_TTL_SECS as i64
                ..=chrono::Utc::now().timestamp() + KNOWN_IP_TTL_SECS as i64)
                .contains(&expires_at)
        );
        let Admission::Allowed(attempt) = throttle.reserve().await.unwrap() else {
            panic!("a recognized address must pass the account pause");
        };
        attempt.refund().await;

        let new_ip = LoginThrottle::for_user("2001:db8:bbbb::/64", &user_id);
        assert!(matches!(
            new_ip.reserve().await.unwrap(),
            Admission::Limited { .. }
        ));

        // Expiration is checked against the stored deadline even if backend
        // cleanup has not yet removed the field.
        store
            .hset(&known.key, &known.field, &0_i64, Some(60))
            .await
            .unwrap();
        assert!(matches!(
            throttle.reserve().await.unwrap(),
            Admission::Limited { .. }
        ));
        record_success(&user_id, known_ip).await;

        for _ in 0..MAX_SUBJECT_IP_ATTEMPTS {
            assert!(matches!(
                throttle.reserve().await.unwrap(),
                Admission::Allowed(_)
            ));
        }
        assert!(
            matches!(throttle.reserve().await.unwrap(), Admission::Limited { retry_after } if retry_after > MAX_BACKOFF_SECS)
        );

        // This recognized pair has no failures of its own. Other accounts
        // still consume its address-wide budget.
        let other_user = ulid::Ulid::generate().to_string();
        record_success(&other_user, known_ip).await;
        let other_pair = LoginThrottle::for_user(known_ip, &other_user);
        for index in 0..MAX_IP_ATTEMPTS - MAX_SUBJECT_IP_ATTEMPTS {
            let identifier = format!("spray-{index}");
            let throttle = LoginThrottle::for_identifier(known_ip, &identifier);
            assert!(matches!(
                throttle.reserve().await.unwrap(),
                Admission::Allowed(_)
            ));
        }
        assert!(
            matches!(other_pair.reserve().await.unwrap(), Admission::Limited { retry_after } if retry_after > MAX_BACKOFF_SECS)
        );
    }

    #[test]
    fn account_backoff_is_progressive_and_bounded() {
        for (count, seconds) in [(5, 2), (6, 4), (7, 8), (8, 16), (9, 30), (40, 30)] {
            let attempts = Attempts {
                count,
                ..Attempts::new(0)
            };
            let policy = Policy::AccountBackoff;
            assert!(
                matches!(policy.decide(attempts, 0), Admission::Limited { retry_after } if retry_after == seconds)
            );
            assert!(matches!(
                policy.decide(attempts, seconds as i64 * 1000),
                Admission::Allowed(_)
            ));
        }
    }
}
