use std::sync::OnceLock;

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};

/// Argon2 hashing configuration: cost parameters plus an optional pepper
/// (applied as the Argon2 `secret`, keyed hashing). Defaults match
/// `argon2::Params::DEFAULT` (Argon2id, m=19456 KiB, t=2, p=1), no pepper.
pub struct HashConfig {
    params: Params,
    pepper: Option<String>,
}

impl HashConfig {
    pub fn new(
        memory_kib: u32,
        iterations: u32,
        parallelism: u32,
        pepper: Option<String>,
    ) -> Result<Self, String> {
        let params = Params::new(memory_kib, iterations, parallelism, None)
            .map_err(|e| format!("invalid argon2 parameters: {e}"))?;
        Ok(Self { params, pepper })
    }

    fn standard() -> Self {
        Self {
            params: Params::default(),
            pepper: None,
        }
    }
}

static CONFIG: OnceLock<HashConfig> = OnceLock::new();

fn config() -> &'static HashConfig {
    CONFIG.get_or_init(HashConfig::standard)
}

/// Successful verification metadata.
pub struct Verification {
    /// The stored hash predates the current configuration (different cost
    /// parameters, or created without the now-configured pepper) and should
    /// be transparently re-encoded now that the plaintext is available.
    pub needs_rehash: bool,
}

pub struct Hash {}

impl Hash {
    /// Install the process-wide hashing configuration. Call once at boot,
    /// before any hashing; later calls fail. Without it, defaults apply.
    pub fn configure(config: HashConfig) -> Result<(), String> {
        CONFIG
            .set(config)
            .map_err(|_| "password hashing already configured".to_string())
    }

    /// Whether a pepper is configured (for boot-time logging).
    pub fn peppered() -> bool {
        config().pepper.is_some()
    }

    /// Configured Argon2 cost parameters as (memory KiB, iterations,
    /// parallelism), for boot-time logging.
    pub fn current_params() -> (u32, u32, u32) {
        let params = &config().params;
        (params.m_cost(), params.t_cost(), params.p_cost())
    }

    /// Encode a string using the configured Argon2 parameters (and pepper,
    /// when set).
    /// # Example
    /// ```rust
    /// use pw::Hash;
    ///
    /// let hashed_value = Hash::encode("my password").unwrap();
    /// let is_valid = Hash::verify("my password", &hashed_value).is_ok();
    ///
    /// assert!(is_valid);
    /// ```
    pub fn encode(value: &str) -> Result<String, argon2::password_hash::Error> {
        Self::encode_with(config(), value)
    }

    pub fn verify(value: &str, hash: &str) -> Result<(), argon2::password_hash::Error> {
        Self::verify_for_login(value, hash).map(|_| ())
    }

    /// Verify and report whether the hash should be re-encoded. When a pepper
    /// is configured and the peppered check fails, an unpeppered check runs
    /// as a migration fallback for hashes that predate the pepper; a match
    /// there verifies successfully with `needs_rehash: true`.
    pub fn verify_for_login(
        value: &str,
        hash: &str,
    ) -> Result<Verification, argon2::password_hash::Error> {
        Self::verify_for_login_with(config(), value, hash)
    }

    /// Whether `hash` was created with different cost parameters than the
    /// current configuration (or is not Argon2id). Unparseable hashes count
    /// as needing a rehash.
    pub fn needs_rehash(hash: &str) -> bool {
        Self::needs_rehash_with(config(), hash)
    }

    /// Whether `value` matches any of the given hashes (unparseable hashes
    /// are skipped). Used for password-history checks; each comparison is a
    /// full Argon2 verification, so callers should treat this as blocking
    /// CPU work proportional to the number of hashes.
    pub fn matches_any<'a, I>(value: &str, hashes: I) -> bool
    where
        I: IntoIterator<Item = &'a str>,
    {
        hashes
            .into_iter()
            .any(|hash| Self::verify(value, hash).is_ok())
    }

    fn encode_with(
        config: &HashConfig,
        value: &str,
    ) -> Result<String, argon2::password_hash::Error> {
        let salt = SaltString::generate(&mut OsRng);
        let hash = hasher(config)?
            .hash_password(value.as_bytes(), &salt)?
            .to_string();
        Ok(hash)
    }

    fn verify_for_login_with(
        config: &HashConfig,
        value: &str,
        hash: &str,
    ) -> Result<Verification, argon2::password_hash::Error> {
        let parsed = PasswordHash::new(hash)?;

        match hasher(config)?.verify_password(value.as_bytes(), &parsed) {
            Ok(()) => Ok(Verification {
                needs_rehash: Self::needs_rehash_with(config, hash),
            }),
            Err(error) => {
                if config.pepper.is_some() {
                    Argon2::default()
                        .verify_password(value.as_bytes(), &parsed)
                        .map(|_| Verification { needs_rehash: true })
                } else {
                    Err(error)
                }
            }
        }
    }

    fn needs_rehash_with(config: &HashConfig, hash: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(hash) else {
            return true;
        };
        if parsed.algorithm != Algorithm::Argon2id.ident() {
            return true;
        }
        let Ok(params) = Params::try_from(&parsed) else {
            return true;
        };
        params.m_cost() != config.params.m_cost()
            || params.t_cost() != config.params.t_cost()
            || params.p_cost() != config.params.p_cost()
    }
}

fn hasher(config: &HashConfig) -> Result<Argon2<'_>, argon2::password_hash::Error> {
    match &config.pepper {
        Some(pepper) => Argon2::new_with_secret(
            pepper.as_bytes(),
            Algorithm::Argon2id,
            Version::V0x13,
            config.params.clone(),
        )
        .map_err(argon2::password_hash::Error::from),
        None => Ok(Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            config.params.clone(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests use the explicit `*_with` internals so they never touch the
    // process-wide OnceLock (test order is nondeterministic).

    fn weak_config(pepper: Option<String>) -> HashConfig {
        // Minimal costs to keep tests fast.
        HashConfig::new(1024, 1, 1, pepper).unwrap()
    }

    #[test]
    fn matches_any_finds_reused_password() {
        let old = Hash::encode("old-password-1!").unwrap();
        let older = Hash::encode("older-password-2!").unwrap();
        let hashes = [old.as_str(), older.as_str(), "not-a-valid-hash"];

        assert!(Hash::matches_any("old-password-1!", hashes));
        assert!(Hash::matches_any("older-password-2!", hashes));
        assert!(!Hash::matches_any("brand-new-password-3!", hashes));
    }

    #[test]
    fn peppered_hash_verifies_and_differs_from_unpeppered() {
        let peppered = weak_config(Some("test-pepper".to_string()));
        let hash = Hash::encode_with(&peppered, "secret1!").unwrap();

        let ok = Hash::verify_for_login_with(&peppered, "secret1!", &hash).unwrap();
        assert!(!ok.needs_rehash);

        // Without the pepper the same hash must not verify.
        let unpeppered = weak_config(None);
        assert!(Hash::verify_for_login_with(&unpeppered, "secret1!", &hash).is_err());
    }

    #[test]
    fn unpeppered_hash_verifies_via_fallback_and_needs_rehash() {
        // Fallback path uses Argon2::default(), so encode with default params.
        let pre_pepper = HashConfig::standard();
        let hash = Hash::encode_with(&pre_pepper, "secret1!").unwrap();

        let peppered = HashConfig::new(
            Params::default().m_cost(),
            Params::default().t_cost(),
            Params::default().p_cost(),
            Some("test-pepper".to_string()),
        )
        .unwrap();
        let ok = Hash::verify_for_login_with(&peppered, "secret1!", &hash).unwrap();
        assert!(ok.needs_rehash);

        // Wrong password still fails through the fallback.
        assert!(Hash::verify_for_login_with(&peppered, "wrong", &hash).is_err());
    }

    #[test]
    fn param_change_triggers_needs_rehash() {
        let old = weak_config(None);
        let hash = Hash::encode_with(&old, "secret1!").unwrap();
        assert!(!Hash::needs_rehash_with(&old, &hash));

        let stronger = HashConfig::new(2048, 2, 1, None).unwrap();
        assert!(Hash::needs_rehash_with(&stronger, &hash));

        let ok = Hash::verify_for_login_with(&stronger, "secret1!", &hash).unwrap();
        assert!(ok.needs_rehash);
    }

    #[test]
    fn garbage_hash_needs_rehash_and_fails_verify() {
        let config = weak_config(None);
        assert!(Hash::needs_rehash_with(&config, "not-a-hash"));
        assert!(Hash::verify_for_login_with(&config, "x", "not-a-hash").is_err());
    }

    #[test]
    fn invalid_params_are_rejected() {
        assert!(HashConfig::new(0, 0, 0, None).is_err());
    }
}
