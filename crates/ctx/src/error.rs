use thiserror::Error;

/// A structural or semantic problem in version 1 claims.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClaimValidationError {
    #[error("{0} must not be blank")]
    Blank(&'static str),
    #[error("{field} exceeds its {max}-byte limit")]
    TooLong {
        field: &'static str,
        max: usize,
        actual: usize,
    },
    #[error("{0} is not a valid uppercase Stargate ULID")]
    InvalidUlid(&'static str),
    #[error("stg.v must be exactly 1")]
    InvalidVersion,
    #[error("actor, authentication, subject, session, and organization claims are inconsistent")]
    InvalidActorMatrix,
    #[error("iat and exp must be non-negative NumericDate values with exp greater than iat")]
    InvalidTimeWindow,
    #[error("token lifetime exceeds 60 seconds")]
    LifetimeTooLong,
    #[error("stg.request.method must be a nonblank ASCII HTTP method")]
    InvalidMethod,
    #[error("{0} must be a nonblank encoded URI path without query or fragment")]
    InvalidPath(&'static str),
    #[error("stg.request.trace_id must contain exactly 32 lowercase hexadecimal characters")]
    InvalidTraceId,
    #[error("stg.request.client_ip must use canonical IPv4 or IPv6 text")]
    InvalidClientIp,
    #[error("stg.dispatch.attempt must be between 1 and 65535")]
    InvalidAttempt,
}

/// Invalid signer or verifier configuration.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    #[error("{0} must not be blank")]
    Blank(&'static str),
    #[error("{field} exceeds its {max}-byte limit")]
    TooLong {
        field: &'static str,
        max: usize,
        actual: usize,
    },
    #[error("kid must contain only printable ASCII characters")]
    InvalidKeyId,
    #[error("token lifetime must be between 1 and 60 seconds")]
    InvalidLifetime,
    #[error("clock skew must not exceed 30 seconds")]
    InvalidClockSkew,
    #[error(transparent)]
    Key(#[from] KeyMaterialError),
}

/// Invalid signing or verification key material.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyMaterialError {
    #[error("invalid RSA key material")]
    InvalidRsa,
    #[error("the verification JWK must contain RSA parameters")]
    NotRsa,
    #[error("the verification JWK is not compatible with RS256 signatures")]
    IncompatibleJwk,
    #[error("RSA keys must be at least 2048 bits")]
    RsaTooSmall,
}

/// A key lookup failure. Resolvers should avoid including unbounded key ids in
/// error text.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyResolutionError {
    #[error("unknown internal-context key")]
    UnknownKey,
    #[error("internal-context key resolver unavailable")]
    Unavailable,
}

/// Internal-context issuance failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IssueError {
    #[error(transparent)]
    Claims(#[from] ClaimValidationError),
    #[error("the configured clock and lifetime overflow NumericDate")]
    TimeOverflow,
    #[error("internal-context signing failed")]
    Signing,
    #[error("the compact internal context exceeds 4096 bytes")]
    TokenTooLarge,
}

/// Missing or duplicate compact-token presentation at an HTTP boundary.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PresentationError {
    #[error("the internal-context header is missing")]
    Missing,
    #[error("the internal-context header must be presented exactly once")]
    Duplicate,
}

/// Low-cardinality verification categories suitable for bounded metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationFailure {
    Compact,
    ProtectedHeader,
    Key,
    Signature,
    Payload,
    Claims,
    Issuer,
    Audience,
    Time,
    Method,
    Path,
    RequestId,
}

/// An internal-context verification failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VerificationError {
    #[error("the compact internal context is malformed")]
    MalformedCompact,
    #[error("the compact internal context exceeds 4096 bytes")]
    TokenTooLarge,
    #[error("the protected JOSE header is invalid")]
    InvalidProtectedHeader,
    #[error("internal-context key resolution failed")]
    KeyResolution,
    #[error("the internal-context signature is invalid")]
    InvalidSignature,
    #[error("the internal-context payload is invalid")]
    InvalidPayload,
    #[error(transparent)]
    Claims(#[from] ClaimValidationError),
    #[error("the internal-context issuer does not match")]
    IssuerMismatch,
    #[error("the internal-context audience does not match")]
    AudienceMismatch,
    #[error("the internal context was issued in the future")]
    IssuedInFuture,
    #[error("the internal context is expired")]
    Expired,
    #[error("the HTTP method does not match the signed context")]
    MethodMismatch,
    #[error("the encoded HTTP path does not match the signed context")]
    PathMismatch,
    #[error("x-request-id does not match the signed context")]
    RequestIdMismatch,
}

impl VerificationError {
    /// Returns a bounded reason category without exposing token contents.
    #[must_use]
    pub const fn category(&self) -> VerificationFailure {
        match self {
            Self::MalformedCompact | Self::TokenTooLarge => VerificationFailure::Compact,
            Self::InvalidProtectedHeader => VerificationFailure::ProtectedHeader,
            Self::KeyResolution => VerificationFailure::Key,
            Self::InvalidSignature => VerificationFailure::Signature,
            Self::InvalidPayload => VerificationFailure::Payload,
            Self::Claims(_) => VerificationFailure::Claims,
            Self::IssuerMismatch => VerificationFailure::Issuer,
            Self::AudienceMismatch => VerificationFailure::Audience,
            Self::IssuedInFuture | Self::Expired => VerificationFailure::Time,
            Self::MethodMismatch => VerificationFailure::Method,
            Self::PathMismatch => VerificationFailure::Path,
            Self::RequestIdMismatch => VerificationFailure::RequestId,
        }
    }
}
