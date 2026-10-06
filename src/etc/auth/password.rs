//! Async wrappers around the CPU-bound Argon2 password primitives in `pw`.
//!
//! Argon2 hashing/verification is memory-hard and takes milliseconds of
//! blocking CPU. Calling it directly on a Tokio worker thread stalls the
//! async runtime and can starve unrelated tasks (including gateway proxying)
//! under concurrent auth load. These helpers offload the work to the blocking
//! pool via `spawn_blocking`.

use std::sync::LazyLock;

use pw::Hash;

/// Install the Argon2 configuration from `ARGON2_*` / `PASSWORD_PEPPER` env
/// vars. Call once at boot (before the CLI dispatch, so `admin bootstrap`
/// hashes with the same configuration as the server); invalid values panic
/// with a clear message.
pub fn init() {
    let memory_kib: u32 = crate::etc::env::parse_or("ARGON2_MEMORY_KIB", 19_456)
        .unwrap_or_else(|error| panic!("{error}"));
    let iterations: u32 =
        crate::etc::env::parse_or("ARGON2_ITERATIONS", 2).unwrap_or_else(|error| panic!("{error}"));
    let parallelism: u32 = crate::etc::env::parse_or("ARGON2_PARALLELISM", 1)
        .unwrap_or_else(|error| panic!("{error}"));
    let pepper = crate::etc::env::optional_string("PASSWORD_PEPPER")
        .filter(|value| !value.trim().is_empty());

    let config = pw::HashConfig::new(memory_kib, iterations, parallelism, pepper)
        .unwrap_or_else(|error| panic!("invalid ARGON2_* configuration: {error}"));
    Hash::configure(config).expect("password hashing configured twice");

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
/// error) yield `valid: false`.
pub async fn check_password(password: String, hash: String) -> PasswordCheck {
    match tokio::task::spawn_blocking(move || {
        Hash::verify_for_login(&password, &hash).map(|verification| verification.needs_rehash)
    })
    .await
    {
        Ok(Ok(needs_rehash)) => PasswordCheck {
            valid: true,
            needs_rehash,
        },
        Ok(Err(_)) => PasswordCheck {
            valid: false,
            needs_rehash: false,
        },
        Err(error) => {
            tracing::error!("password verification task failed: {error}");
            PasswordCheck {
                valid: false,
                needs_rehash: false,
            }
        }
    }
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
/// Returns `true` iff the password matches. Any failure — malformed hash,
/// mismatch, or a join error — returns `false` (deny).
pub async fn verify_password(password: String, hash: String) -> bool {
    match tokio::task::spawn_blocking(move || Hash::verify(&password, &hash).is_ok()).await {
        Ok(ok) => ok,
        Err(error) => {
            tracing::error!("password verification task failed: {error}");
            false
        }
    }
}

/// Hash a password with Argon2 on the blocking pool.
///
/// Returns `None` if hashing fails (rare — e.g. RNG failure) or the task
/// panics; callers map this to their own error type.
pub async fn hash_password(password: String) -> Option<String> {
    match tokio::task::spawn_blocking(move || Hash::encode(&password)).await {
        Ok(Ok(hash)) => Some(hash),
        Ok(Err(error)) => {
            tracing::error!("password hashing failed: {error}");
            None
        }
        Err(error) => {
            tracing::error!("password hashing task failed: {error}");
            None
        }
    }
}

/// Return a stable dummy Argon2 hash for constant-time login on miss paths.
pub fn dummy_hash() -> String {
    DUMMY_HASH.clone()
}
