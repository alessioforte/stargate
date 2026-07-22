use std::fmt;
use std::sync::Arc;

use jsonwebtoken::{Algorithm, Header, encode};

use crate::claims::{
    IssueRequest, MAX_ISSUER_BYTES, MAX_KEY_ID_BYTES, MAX_LIFETIME_SECS, MAX_TOKEN_BYTES,
    WireClaims, validate_claims,
};
use crate::error::{ConfigError, IssueError};
use crate::keys::parse_private_key;
use crate::source::{Clock, DispatchIdSource, SystemClock, UlidDispatchIdSource};

pub const JOSE_TYPE: &str = "stargate-context+jwt";

/// Immutable version 1 signer configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerConfig {
    issuer: String,
    key_id: String,
    lifetime_secs: i64,
}

impl SignerConfig {
    pub fn new(
        issuer: impl Into<String>,
        key_id: impl Into<String>,
        lifetime_secs: u64,
    ) -> Result<Self, ConfigError> {
        let issuer = issuer.into();
        let key_id = key_id.into();
        validate_nonblank_config("issuer", &issuer)?;
        validate_config_len("issuer", &issuer, MAX_ISSUER_BYTES)?;
        validate_key_id(&key_id)?;
        if lifetime_secs == 0 || lifetime_secs > MAX_LIFETIME_SECS as u64 {
            return Err(ConfigError::InvalidLifetime);
        }
        Ok(Self {
            issuer,
            key_id,
            lifetime_secs: lifetime_secs as i64,
        })
    }

    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    #[must_use]
    pub const fn lifetime_secs(&self) -> i64 {
        self.lifetime_secs
    }
}

/// One signed compact context. Debug output is always redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct IssuedContext {
    compact: String,
}

impl IssuedContext {
    #[must_use]
    pub fn compact(&self) -> &str {
        &self.compact
    }

    #[must_use]
    pub fn into_compact(self) -> String {
        self.compact
    }
}

impl fmt::Debug for IssuedContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IssuedContext([redacted])")
    }
}

/// RS256 issuer for the frozen version 1 internal-context profile.
pub struct ContextSigner {
    config: SignerConfig,
    encoding_key: jsonwebtoken::EncodingKey,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn DispatchIdSource>,
}

impl ContextSigner {
    pub fn from_rsa_pem(config: SignerConfig, private_key_pem: &[u8]) -> Result<Self, ConfigError> {
        Self::with_sources(
            config,
            private_key_pem,
            Arc::new(SystemClock),
            Arc::new(UlidDispatchIdSource),
        )
    }

    pub fn with_sources(
        config: SignerConfig,
        private_key_pem: &[u8],
        clock: Arc<dyn Clock>,
        ids: Arc<dyn DispatchIdSource>,
    ) -> Result<Self, ConfigError> {
        let encoding_key = parse_private_key(private_key_pem)?;
        Ok(Self {
            config,
            encoding_key,
            clock,
            ids,
        })
    }

    pub fn issue(&self, request: &IssueRequest) -> Result<IssuedContext, IssueError> {
        let issued_at = self.clock.unix_timestamp();
        let expires_at = issued_at
            .checked_add(self.config.lifetime_secs)
            .ok_or(IssueError::TimeOverflow)?;
        let claims = WireClaims {
            iss: self.config.issuer.clone(),
            aud: request.audience.clone(),
            iat: issued_at,
            exp: expires_at,
            jti: self.ids.next_dispatch_id(),
            sub: request.subject.clone(),
            stg: request.context.clone(),
        };
        validate_claims(&claims)?;

        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some(JOSE_TYPE.to_owned());
        header.kid = Some(self.config.key_id.clone());
        let compact =
            encode(&header, &claims, &self.encoding_key).map_err(|_| IssueError::Signing)?;
        if compact.len() > MAX_TOKEN_BYTES {
            return Err(IssueError::TokenTooLarge);
        }
        Ok(IssuedContext { compact })
    }
}

impl fmt::Debug for ContextSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContextSigner")
            .field("issuer", &self.config.issuer)
            .field("key_id", &self.config.key_id)
            .field("lifetime_secs", &self.config.lifetime_secs)
            .field("encoding_key", &"[redacted]")
            .finish_non_exhaustive()
    }
}

pub(crate) fn validate_key_id(key_id: &str) -> Result<(), ConfigError> {
    validate_nonblank_config("kid", key_id)?;
    validate_config_len("kid", key_id, MAX_KEY_ID_BYTES)?;
    if !key_id.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(ConfigError::InvalidKeyId);
    }
    Ok(())
}

pub(crate) fn validate_nonblank_config(
    field: &'static str,
    value: &str,
) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::Blank(field));
    }
    Ok(())
}

pub(crate) fn validate_config_len(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<(), ConfigError> {
    if value.len() > max {
        return Err(ConfigError::TooLong {
            field,
            max,
            actual: value.len(),
        });
    }
    Ok(())
}
