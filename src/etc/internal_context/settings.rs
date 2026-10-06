use super::{InternalContextError, InternalContextRuntime, signing};
use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) const ALGORITHM_ENV: &str = "INTERNAL_CONTEXT_ALGORITHM";
pub(super) const ISSUER_ENV: &str = "INTERNAL_CONTEXT_ISSUER";
pub(super) const KEY_ID_ENV: &str = "INTERNAL_CONTEXT_KID";
pub(super) const PRIVATE_KEY_PATH_ENV: &str = "INTERNAL_CONTEXT_PRIVATE_KEY_PATH";
pub(super) const JWKS_PATH_ENV: &str = "INTERNAL_CONTEXT_JWKS_PATH";
pub(super) const TTL_ENV: &str = "INTERNAL_CONTEXT_TTL_SECS";
pub(super) const CLOCK_SKEW_ENV: &str = "INTERNAL_CONTEXT_CLOCK_SKEW_SECS";
pub(super) const CACHE_MAX_AGE_ENV: &str = "INTERNAL_CONTEXT_JWKS_CACHE_MAX_AGE_SECS";

pub(super) const DEFAULT_TTL_SECS: u64 = 30;
pub(super) const DEFAULT_CLOCK_SKEW_SECS: u64 = 5;
pub(super) const DEFAULT_CACHE_MAX_AGE_SECS: u64 = 60;

#[derive(Debug)]
pub(super) struct RuntimeSettings {
    pub(super) signing: signing::Settings,
    pub(super) issuer: String,
    pub(super) key_id: String,
    private_key_path: PathBuf,
    jwks_path: PathBuf,
    pub(super) ttl_secs: u64,
    pub(super) clock_skew_secs: u64,
    pub(super) cache_max_age_secs: u64,
}

impl RuntimeSettings {
    fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<Self>, InternalContextError> {
        let names = [
            ALGORITHM_ENV,
            ISSUER_ENV,
            KEY_ID_ENV,
            PRIVATE_KEY_PATH_ENV,
            JWKS_PATH_ENV,
            TTL_ENV,
            CLOCK_SKEW_ENV,
            CACHE_MAX_AGE_ENV,
            signing::WORKERS_ENV,
            signing::QUEUE_ENV,
            signing::TIMEOUT_ENV,
        ];
        if !names.iter().any(|name| lookup(name).is_some()) {
            return Ok(None);
        }

        let algorithm = lookup(ALGORITHM_ENV).unwrap_or_else(|| "RS256".to_owned());
        if algorithm != "RS256" {
            return Err(InternalContextError::UnsupportedAlgorithm);
        }

        Ok(Some(Self {
            signing: signing::Settings::from_lookup(&lookup)?,
            issuer: required(&lookup, ISSUER_ENV)?,
            key_id: required(&lookup, KEY_ID_ENV)?,
            private_key_path: PathBuf::from(required(&lookup, PRIVATE_KEY_PATH_ENV)?),
            jwks_path: PathBuf::from(required(&lookup, JWKS_PATH_ENV)?),
            ttl_secs: unsigned_or_default(&lookup, TTL_ENV, DEFAULT_TTL_SECS)?,
            clock_skew_secs: unsigned_or_default(&lookup, CLOCK_SKEW_ENV, DEFAULT_CLOCK_SKEW_SECS)?,
            cache_max_age_secs: unsigned_or_default(
                &lookup,
                CACHE_MAX_AGE_ENV,
                DEFAULT_CACHE_MAX_AGE_SECS,
            )?,
        }))
    }
}

pub(super) fn load_from_env() -> Result<Option<Arc<InternalContextRuntime>>, InternalContextError> {
    load_with(|name| env::var(name).ok(), |path| std::fs::read(path))
}

fn load_with(
    lookup: impl Fn(&str) -> Option<String>,
    mut read: impl FnMut(&Path) -> Result<Vec<u8>, std::io::Error>,
) -> Result<Option<Arc<InternalContextRuntime>>, InternalContextError> {
    let Some(settings) = RuntimeSettings::from_lookup(lookup)? else {
        return Ok(None);
    };
    let private_key =
        read(&settings.private_key_path).map_err(|source| InternalContextError::ReadFile {
            kind: "private key",
            path: settings.private_key_path.clone(),
            source,
        })?;
    let jwks = read(&settings.jwks_path).map_err(|source| InternalContextError::ReadFile {
        kind: "JWKS",
        path: settings.jwks_path.clone(),
        source,
    })?;
    Ok(Some(Arc::new(InternalContextRuntime::from_material(
        settings,
        &private_key,
        &jwks,
    )?)))
}

fn required(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &'static str,
) -> Result<String, InternalContextError> {
    lookup(name)
        .filter(|value| !value.trim().is_empty())
        .ok_or(InternalContextError::MissingSetting(name))
}

pub(super) fn unsigned_or_default(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &'static str,
    default: u64,
) -> Result<u64, InternalContextError> {
    lookup(name)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| InternalContextError::InvalidInteger { name })
        })
        .unwrap_or(Ok(default))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) mod test_support;
