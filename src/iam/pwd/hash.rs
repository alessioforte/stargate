//! Async wrappers around the CPU-bound Argon2 password primitives in `pw`.
//!
//! Argon2 hashing/verification is memory-hard and takes milliseconds of
//! blocking CPU. Calling it directly on a Tokio worker thread stalls the
//! async runtime and can starve unrelated tasks (including gateway proxying)
//! under concurrent auth load. These helpers offload the work to the blocking
//! pool via `spawn_blocking`.

use std::sync::LazyLock;

use pw::Hash;
mod budget;
#[cfg(test)]
pub(crate) use budget::exhausted as with_exhausted_budget;

#[derive(Debug, thiserror::Error)]
pub enum HashError {
    #[error("password hashing is busy")]
    Overloaded,
    #[error("password hashing failed")]
    Failed,
}
impl From<HashError> for crate::err::ErrorResponse {
    fn from(error: HashError) -> Self {
        match error {
            HashError::Overloaded => Self::new(crate::err::ErrorCode::IamOverloaded),
            HashError::Failed => Self::internal(error),
        }
    }
}

/// Install the Argon2 configuration from `ARGON2_*` / `PASSWORD_PEPPER` env
/// vars. Call once at boot (before the CLI dispatch, so `admin bootstrap`
/// hashes with the same configuration as the server); invalid values panic
/// with a clear message.
pub fn init() {
    let memory_kib: u32 = crate::conf::env::parse_or("ARGON2_MEMORY_KIB", 19_456)
        .unwrap_or_else(|error| panic!("{error}"));
    let iterations: u32 = crate::conf::env::parse_or("ARGON2_ITERATIONS", 2)
        .unwrap_or_else(|error| panic!("{error}"));
    let parallelism: u32 = crate::conf::env::parse_or("ARGON2_PARALLELISM", 1)
        .unwrap_or_else(|error| panic!("{error}"));
    let pepper = crate::conf::env::optional_string("PASSWORD_PEPPER")
        .filter(|value| !value.trim().is_empty());

    let config = pw::HashConfig::new(memory_kib, iterations, parallelism, pepper)
        .unwrap_or_else(|error| panic!("invalid ARGON2_* configuration: {error}"));
    Hash::configure(config).expect("password hashing configured twice");

    let _budget = budget::current();
    LazyLock::force(&DUMMY_HASH);

    let (m, t, p) = Hash::current_params();
    tracing::info!(
        "Password hashing: argon2id m={}KiB t={} p={} pepper={}",
        m,
        t,
        p,
        Hash::peppered(),
    );
}

/// Outcome of a password check against a stored hash.
pub struct PasswordCheck {
    pub valid: bool,
    /// The stored hash predates the current Argon2 configuration (cost
    /// change or newly configured pepper) and should be re-encoded while the
    /// plaintext is at hand.
    pub needs_rehash: bool,
}

/// Verify a password on the blocking pool, reporting whether the stored hash
/// needs a transparent upgrade. Failures (mismatch, malformed hash, join
/// error) are distinct from admission failures.
pub async fn check_password(password: String, hash: String) -> Result<PasswordCheck, HashError> {
    budget::current()
        .run("verify", move || {
            match Hash::verify_for_login(&password, &hash) {
                Ok(verification) => PasswordCheck {
                    valid: true,
                    needs_rehash: verification.needs_rehash,
                },
                Err(_) => PasswordCheck {
                    valid: false,
                    needs_rehash: false,
                },
            }
        })
        .await
}

/// Fixed hash used to equalize timing on login paths where no stored
/// credential exists (unknown user / missing credential). Verifying a
/// supplied password against this hash makes the miss path take
/// approximately as long as the valid-user path, preventing username
/// enumeration via response timing.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    Hash::encode("stargate::login::timing::equalizer")
        .expect("failed to precompute dummy password hash")
});

/// Verify a password against an Argon2 hash on the blocking pool.
///
/// Mismatches and malformed hashes return false; admission errors remain distinct.
pub async fn verify_password(password: String, hash: String) -> Result<bool, HashError> {
    budget::current()
        .run("verify", move || Hash::verify(&password, &hash).is_ok())
        .await
}

/// Hash on the bounded blocking pool, with a five-second admission deadline.
pub async fn hash_password(password: String) -> Result<String, HashError> {
    budget::current()
        .run("hash", move || Hash::encode(&password))
        .await?
        .map_err(|error| {
            tracing::error!(%error, "password hashing failed");
            HashError::Failed
        })
}

/// Best-effort background upgrade: never wait ahead of foreground password work.
pub async fn try_rehash_password(password: String) -> Result<Option<String>, HashError> {
    budget::current()
        .rehash(move || Hash::encode(&password))
        .await?
        .transpose()
        .map_err(|error| {
            tracing::error!(%error, "password rehash failed");
            HashError::Failed
        })
}

/// Verify sequential history entries while holding one process-wide permit.
pub async fn matches_history(password: String, hashes: Vec<String>) -> Result<bool, HashError> {
    budget::current()
        .run("history", move || {
            Hash::matches_any(&password, hashes.iter().map(String::as_str))
        })
        .await
}

/// Startup precomputes this hash before any request is accepted.
pub fn dummy_hash() -> String {
    DUMMY_HASH.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn overload_is_not_a_mismatch_and_background_rehash_skips() {
        with_exhausted_budget(async {
            assert!(matches!(
                check_password("pw".into(), "hash".into()).await,
                Err(HashError::Overloaded)
            ));
            assert!(matches!(
                verify_password("pw".into(), "hash".into()).await,
                Err(HashError::Overloaded)
            ));
            assert!(matches!(
                hash_password("pw".into()).await,
                Err(HashError::Overloaded)
            ));
            assert!(matches!(
                matches_history("pw".into(), vec!["hash".into()]).await,
                Err(HashError::Overloaded)
            ));
            assert!(try_rehash_password("pw".into()).await.unwrap().is_none());
        })
        .await;
    }
    #[test]
    fn startup_validates_concurrency_and_precomputes_the_dummy_hash() {
        const CHILD: &str = "STARGATE_TEST_PASSWORD_HASH_BOOT";
        if std::env::var_os(CHILD).is_some() {
            init();
            assert!(LazyLock::get(&DUMMY_HASH).is_some());
            return;
        }
        for (value, success) in [
            (None, true),
            (Some("2"), true),
            (Some("0"), false),
            (Some(""), false),
            (Some("-1"), false),
            (Some("invalid"), false),
        ] {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "iam::pwd::hash::tests::startup_validates_concurrency_and_precomputes_the_dummy_hash", "--nocapture"])
                .env(CHILD, "1").env("ARGON2_MEMORY_KIB", "8").env("ARGON2_ITERATIONS", "1").env("ARGON2_PARALLELISM", "1").env_remove("PASSWORD_PEPPER");
            if let Some(value) = value {
                command.env("PASSWORD_HASH_CONCURRENCY", value);
            } else {
                command.env_remove("PASSWORD_HASH_CONCURRENCY");
            }
            let output = command.output().unwrap();
            assert_eq!(
                output.status.success(),
                success,
                "{value:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            if !success {
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains("PASSWORD_HASH_CONCURRENCY")
                );
            }
        }
    }
}
