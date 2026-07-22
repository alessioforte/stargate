use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::{Algorithm, Validation, decode};

use crate::claims::{
    MAX_AUDIENCE_BYTES, MAX_CLOCK_SKEW_SECS, MAX_ISSUER_BYTES, MAX_TOKEN_BYTES, TrustedContext,
    WireClaims, validate_claims,
};
use crate::error::{ConfigError, PresentationError, VerificationError};
use crate::keys::KeyResolver;
use crate::signer::{JOSE_TYPE, validate_config_len, validate_key_id, validate_nonblank_config};
use crate::source::{Clock, SystemClock};
use crate::strict_json;

/// Immutable consumer trust configuration for one exact issuer and audience.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifierConfig {
    issuer: String,
    audience: String,
    clock_skew_secs: u64,
}

impl VerifierConfig {
    pub fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        clock_skew_secs: u64,
    ) -> Result<Self, ConfigError> {
        let issuer = issuer.into();
        let audience = audience.into();
        validate_nonblank_config("issuer", &issuer)?;
        validate_config_len("issuer", &issuer, MAX_ISSUER_BYTES)?;
        validate_nonblank_config("audience", &audience)?;
        validate_config_len("audience", &audience, MAX_AUDIENCE_BYTES)?;
        if clock_skew_secs > MAX_CLOCK_SKEW_SECS {
            return Err(ConfigError::InvalidClockSkew);
        }
        Ok(Self {
            issuer,
            audience,
            clock_skew_secs,
        })
    }

    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    #[must_use]
    pub fn audience(&self) -> &str {
        &self.audience
    }

    #[must_use]
    pub const fn clock_skew_secs(&self) -> u64 {
        self.clock_skew_secs
    }
}

/// Plain HTTP facts that must match the signed request binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedRequest<'a> {
    pub method: &'a str,
    pub encoded_path: &'a str,
    pub request_id: &'a str,
}

/// Verifies one compact internal context and constructs the only trusted type
/// exposed by this crate.
pub struct ContextVerifier {
    config: VerifierConfig,
    resolver: Arc<dyn KeyResolver>,
    clock: Arc<dyn Clock>,
}

impl ContextVerifier {
    #[must_use]
    pub fn new(config: VerifierConfig, resolver: Arc<dyn KeyResolver>) -> Self {
        Self::with_clock(config, resolver, Arc::new(SystemClock))
    }

    #[must_use]
    pub fn with_clock(
        config: VerifierConfig,
        resolver: Arc<dyn KeyResolver>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            config,
            resolver,
            clock,
        }
    }

    pub fn verify(
        &self,
        compact: &str,
        expected: ExpectedRequest<'_>,
    ) -> Result<TrustedContext, VerificationError> {
        if compact.len() > MAX_TOKEN_BYTES {
            return Err(VerificationError::TokenTooLarge);
        }
        let segments = compact_segments(compact)?;
        let key_id = validate_protected_header(segments[0])?;
        let key = self
            .resolver
            .resolve(&key_id)
            .map_err(|_| VerificationError::KeyResolution)?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.required_spec_claims.clear();
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.validate_aud = false;
        decode::<serde_json::Value>(compact, &key.decoding_key, &validation)
            .map_err(map_decode_error)?;

        let payload = decode_segment(segments[1]).map_err(|_| VerificationError::InvalidPayload)?;
        let value = strict_json::parse(&payload).map_err(|_| VerificationError::InvalidPayload)?;
        let claims: WireClaims =
            serde_json::from_value(value).map_err(|_| VerificationError::InvalidPayload)?;
        validate_claims(&claims)?;

        if claims.iss != self.config.issuer {
            return Err(VerificationError::IssuerMismatch);
        }
        if claims.aud != self.config.audience {
            return Err(VerificationError::AudienceMismatch);
        }

        let now = self.clock.unix_timestamp();
        let skew = i64::try_from(self.config.clock_skew_secs).unwrap_or(i64::MAX);
        if claims.iat > now.saturating_add(skew) {
            return Err(VerificationError::IssuedInFuture);
        }
        if claims.exp <= now.saturating_sub(skew) {
            return Err(VerificationError::Expired);
        }
        if claims.stg.request.method != expected.method {
            return Err(VerificationError::MethodMismatch);
        }
        if claims.stg.request.path != expected.encoded_path {
            return Err(VerificationError::PathMismatch);
        }
        if claims.stg.request.id != expected.request_id {
            return Err(VerificationError::RequestIdMismatch);
        }

        Ok(TrustedContext::new(claims))
    }
}

/// Selects exactly one presented field value without depending on an HTTP
/// framework. P6 adapters can feed `HeaderMap::get_all` into this helper.
pub fn require_single_token<'a, I>(values: I) -> Result<&'a str, PresentationError>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut values = values.into_iter();
    let first = values.next().ok_or(PresentationError::Missing)?;
    if values.next().is_some() {
        return Err(PresentationError::Duplicate);
    }
    Ok(first)
}

fn compact_segments(compact: &str) -> Result<[&str; 3], VerificationError> {
    let mut segments = compact.split('.');
    let header = segments.next().ok_or(VerificationError::MalformedCompact)?;
    let payload = segments.next().ok_or(VerificationError::MalformedCompact)?;
    let signature = segments.next().ok_or(VerificationError::MalformedCompact)?;
    if header.is_empty() || payload.is_empty() || signature.is_empty() || segments.next().is_some()
    {
        return Err(VerificationError::MalformedCompact);
    }
    Ok([header, payload, signature])
}

fn validate_protected_header(segment: &str) -> Result<String, VerificationError> {
    let decoded = decode_segment(segment).map_err(|_| VerificationError::InvalidProtectedHeader)?;
    let value =
        strict_json::parse(&decoded).map_err(|_| VerificationError::InvalidProtectedHeader)?;
    let object = value
        .as_object()
        .ok_or(VerificationError::InvalidProtectedHeader)?;
    if object.len() != 3
        || !object.contains_key("alg")
        || !object.contains_key("kid")
        || !object.contains_key("typ")
    {
        return Err(VerificationError::InvalidProtectedHeader);
    }
    if object.get("alg").and_then(|value| value.as_str()) != Some("RS256")
        || object.get("typ").and_then(|value| value.as_str()) != Some(JOSE_TYPE)
    {
        return Err(VerificationError::InvalidProtectedHeader);
    }
    let key_id = object
        .get("kid")
        .and_then(|value| value.as_str())
        .ok_or(VerificationError::InvalidProtectedHeader)?;
    validate_key_id(key_id).map_err(|_| VerificationError::InvalidProtectedHeader)?;
    Ok(key_id.to_owned())
}

fn decode_segment(segment: &str) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(segment)
}

fn map_decode_error(error: jsonwebtoken::errors::Error) -> VerificationError {
    match error.kind() {
        ErrorKind::InvalidSignature => VerificationError::InvalidSignature,
        ErrorKind::InvalidAlgorithm | ErrorKind::MissingAlgorithm => {
            VerificationError::InvalidProtectedHeader
        }
        ErrorKind::Json(_) | ErrorKind::Utf8(_) => VerificationError::InvalidPayload,
        _ => VerificationError::MalformedCompact,
    }
}
