use super::*;

pub(in crate::etc::internal_context) fn settings(key_id: &str) -> RuntimeSettings {
    RuntimeSettings {
        signing: signing::Settings::default(),
        issuer: "https://stargate.test/internal-context".to_owned(),
        key_id: key_id.to_owned(),
        private_key_path: PathBuf::from("private.pem"),
        jwks_path: PathBuf::from("jwks.json"),
        ttl_secs: DEFAULT_TTL_SECS,
        clock_skew_secs: DEFAULT_CLOCK_SKEW_SECS,
        cache_max_age_secs: DEFAULT_CACHE_MAX_AGE_SECS,
    }
}
