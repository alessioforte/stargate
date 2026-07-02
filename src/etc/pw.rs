//! Async wrappers around the CPU-bound Argon2 password primitives in `pw`.
//!
//! Argon2 hashing/verification is memory-hard and takes milliseconds of
//! blocking CPU. Calling it directly on a Tokio worker thread stalls the
//! async runtime and can starve unrelated tasks (including gateway proxying)
//! under concurrent auth load. These helpers offload the work to the blocking
//! pool via `spawn_blocking`.

use std::sync::LazyLock;

use pw::Hash;

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
