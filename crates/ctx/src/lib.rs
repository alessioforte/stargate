//! Pure version 1 contract, issuer, and verifier for Stargate's signed
//! `stargate-context` header.
//!
//! [`IssueRequest`] and its nested structures are unverified issuance inputs.
//! Only [`ContextVerifier::verify`] can construct [`TrustedContext`].

mod claims;
mod error;
mod keys;
mod signer;
mod source;
mod strict_json;
mod verifier;

pub use claims::{
    Actor, ActorType, Authentication, AuthenticationKind, DispatchContext, DispatchKind,
    IssueRequest, MAX_AUDIENCE_BYTES, MAX_CLOCK_SKEW_SECS, MAX_ISSUER_BYTES, MAX_KEY_ID_BYTES,
    MAX_LIFETIME_SECS, MAX_METHOD_BYTES, MAX_NAME_BYTES, MAX_PATH_BYTES, MAX_REQUEST_ID_BYTES,
    MAX_ROLE_BYTES, MAX_TOKEN_BYTES, MAX_USER_AGENT_BYTES, Organization, RequestContext,
    RouteContext, StargateContext, TrustedContext, VERSION, truncate_utf8,
};
pub use error::{
    ClaimValidationError, ConfigError, IssueError, KeyMaterialError, KeyResolutionError,
    PresentationError, VerificationError, VerificationFailure,
};
pub use keys::{KeyResolver, StaticKeyResolver, VerificationKey};
pub use signer::{ContextSigner, IssuedContext, JOSE_TYPE, SignerConfig};
pub use source::{Clock, DispatchIdSource, SystemClock, UlidDispatchIdSource};
pub use verifier::{ContextVerifier, ExpectedRequest, VerifierConfig, require_single_token};
