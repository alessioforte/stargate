use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::error::ClaimValidationError;

pub const VERSION: u8 = 1;
pub const MAX_TOKEN_BYTES: usize = 4_096;
pub const MAX_LIFETIME_SECS: i64 = 60;
pub const MAX_CLOCK_SKEW_SECS: u64 = 30;
pub const MAX_ISSUER_BYTES: usize = 256;
pub const MAX_AUDIENCE_BYTES: usize = 256;
pub const MAX_KEY_ID_BYTES: usize = 128;
pub const MAX_ROLE_BYTES: usize = 256;
pub const MAX_NAME_BYTES: usize = 128;
pub const MAX_PATH_BYTES: usize = 2_048;
pub const MAX_USER_AGENT_BYTES: usize = 512;
pub const MAX_REQUEST_ID_BYTES: usize = 64;
pub const MAX_METHOD_BYTES: usize = 32;

/// Truncates a UTF-8 string to at most `max_bytes` without splitting a code
/// point. The returned slice borrows the original input.
#[must_use]
pub fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

/// The effective actor selected by Stargate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    User,
    ApiKey,
    Anonymous,
}

/// The authentication mechanism that produced the effective actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationKind {
    Jwt,
    ApiKey,
    None,
}

/// Whether this is primary traffic or an isolated mirror dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchKind {
    Primary,
    Shadow,
}

/// Unverified actor facts supplied to the signer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    #[serde(rename = "type")]
    pub actor_type: ActorType,
}

/// Unverified authentication facts supplied to the signer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authentication {
    pub kind: AuthenticationKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_time: Option<i64>,
}

/// An asserted active organization, not persisted-resource ownership proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Organization {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// Bound request and audit-input facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestContext {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    pub method: String,
    pub path: String,
    pub original_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
}

/// Diagnostic route-selection facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteContext {
    pub router: String,
    pub service: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_revision: Option<String>,
}

/// Per-network-attempt dispatch facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchContext {
    pub kind: DispatchKind,
    pub attempt: u16,
}

/// The private, versioned `stg` namespace supplied to the signer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StargateContext {
    pub v: u8,
    pub actor: Actor,
    pub authentication: Authentication,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<Organization>,
    pub request: RequestContext,
    pub route: RouteContext,
    pub dispatch: DispatchContext,
}

/// Unverified facts used to issue one internal context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRequest {
    pub audience: String,
    pub subject: Option<String>,
    pub context: StargateContext,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireClaims {
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    pub stg: StargateContext,
}

/// Claims that have passed signature, schema, time, and request-binding
/// validation. This type intentionally does not implement `Deserialize` and
/// can only be created by [`crate::ContextVerifier`].
#[derive(Clone, PartialEq, Eq)]
pub struct TrustedContext {
    claims: WireClaims,
}

impl TrustedContext {
    pub(crate) fn new(claims: WireClaims) -> Self {
        Self { claims }
    }

    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.claims.iss
    }

    #[must_use]
    pub fn audience(&self) -> &str {
        &self.claims.aud
    }

    #[must_use]
    pub const fn issued_at(&self) -> i64 {
        self.claims.iat
    }

    #[must_use]
    pub const fn expires_at(&self) -> i64 {
        self.claims.exp
    }

    #[must_use]
    pub fn dispatch_id(&self) -> &str {
        &self.claims.jti
    }

    #[must_use]
    pub fn subject(&self) -> Option<&str> {
        self.claims.sub.as_deref()
    }

    #[must_use]
    pub const fn context(&self) -> &StargateContext {
        &self.claims.stg
    }
}

impl fmt::Debug for TrustedContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TrustedContext([redacted])")
    }
}

pub(crate) fn validate_claims(claims: &WireClaims) -> Result<(), ClaimValidationError> {
    validate_nonblank("iss", &claims.iss)?;
    validate_len("iss", &claims.iss, MAX_ISSUER_BYTES)?;
    validate_nonblank("aud", &claims.aud)?;
    validate_len("aud", &claims.aud, MAX_AUDIENCE_BYTES)?;
    validate_ulid("jti", &claims.jti)?;

    if claims.iat < 0 || claims.exp < 0 || claims.exp <= claims.iat {
        return Err(ClaimValidationError::InvalidTimeWindow);
    }
    if claims.exp - claims.iat > MAX_LIFETIME_SECS {
        return Err(ClaimValidationError::LifetimeTooLong);
    }
    if claims.stg.v != VERSION {
        return Err(ClaimValidationError::InvalidVersion);
    }

    validate_actor_matrix(claims)?;
    validate_request(&claims.stg.request)?;
    validate_route(&claims.stg.route)?;
    if claims.stg.dispatch.attempt == 0 {
        return Err(ClaimValidationError::InvalidAttempt);
    }

    Ok(())
}

fn validate_actor_matrix(claims: &WireClaims) -> Result<(), ClaimValidationError> {
    let actor = claims.stg.actor.actor_type;
    let auth = &claims.stg.authentication;
    let organization = claims.stg.organization.as_ref();

    let valid = match actor {
        ActorType::User => {
            auth.kind == AuthenticationKind::Jwt
                && claims.sub.as_deref().is_some_and(is_valid_ulid)
                && auth.sid.as_deref().is_some_and(is_valid_ulid)
                && auth.auth_time.is_none_or(|value| value >= 0)
                && organization.is_none_or(|organization| organization.role.is_some())
        }
        ActorType::ApiKey => {
            auth.kind == AuthenticationKind::ApiKey
                && claims.sub.as_deref().is_some_and(is_valid_ulid)
                && auth.sid.is_none()
                && auth.auth_time.is_none()
        }
        ActorType::Anonymous => {
            auth.kind == AuthenticationKind::None
                && claims.sub.is_none()
                && auth.sid.is_none()
                && auth.auth_time.is_none()
                && organization.is_none()
        }
    };

    if !valid {
        return Err(ClaimValidationError::InvalidActorMatrix);
    }

    if let Some(organization) = organization {
        validate_ulid("stg.organization.id", &organization.id)?;
        if let Some(role) = organization.role.as_deref() {
            validate_nonblank("stg.organization.role", role)?;
            validate_len("stg.organization.role", role, MAX_ROLE_BYTES)?;
        }
    }

    Ok(())
}

fn validate_request(request: &RequestContext) -> Result<(), ClaimValidationError> {
    validate_ulid("stg.request.id", &request.id)?;
    if request.id.len() > MAX_REQUEST_ID_BYTES {
        return Err(ClaimValidationError::TooLong {
            field: "stg.request.id",
            max: MAX_REQUEST_ID_BYTES,
            actual: request.id.len(),
        });
    }

    if request.method.is_empty()
        || request.method.len() > MAX_METHOD_BYTES
        || !request.method.is_ascii()
        || request
            .method
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return Err(ClaimValidationError::InvalidMethod);
    }

    validate_path("stg.request.path", &request.path)?;
    validate_path("stg.request.original_path", &request.original_path)?;

    if let Some(trace_id) = request.trace_id.as_deref()
        && (trace_id.len() != 32
            || !trace_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    {
        return Err(ClaimValidationError::InvalidTraceId);
    }

    if let Some(client_ip) = request.client_ip.as_deref() {
        let parsed =
            IpAddr::from_str(client_ip).map_err(|_| ClaimValidationError::InvalidClientIp)?;
        if parsed.to_string() != client_ip {
            return Err(ClaimValidationError::InvalidClientIp);
        }
    }

    if let Some(user_agent) = request.user_agent.as_deref() {
        validate_len("stg.request.user_agent", user_agent, MAX_USER_AGENT_BYTES)?;
    }

    Ok(())
}

fn validate_route(route: &RouteContext) -> Result<(), ClaimValidationError> {
    validate_nonblank("stg.route.router", &route.router)?;
    validate_len("stg.route.router", &route.router, MAX_NAME_BYTES)?;
    validate_nonblank("stg.route.service", &route.service)?;
    validate_len("stg.route.service", &route.service, MAX_NAME_BYTES)?;
    if let Some(revision) = route.policy_revision.as_deref() {
        validate_nonblank("stg.route.policy_revision", revision)?;
        validate_len("stg.route.policy_revision", revision, MAX_NAME_BYTES)?;
    }
    Ok(())
}

fn validate_path(field: &'static str, path: &str) -> Result<(), ClaimValidationError> {
    if path.is_empty()
        || !path.starts_with('/')
        || path.contains(['?', '#'])
        || path.len() > MAX_PATH_BYTES
    {
        return Err(ClaimValidationError::InvalidPath(field));
    }
    Ok(())
}

fn validate_nonblank(field: &'static str, value: &str) -> Result<(), ClaimValidationError> {
    if value.trim().is_empty() {
        return Err(ClaimValidationError::Blank(field));
    }
    Ok(())
}

fn validate_len(field: &'static str, value: &str, max: usize) -> Result<(), ClaimValidationError> {
    if value.len() > max {
        return Err(ClaimValidationError::TooLong {
            field,
            max,
            actual: value.len(),
        });
    }
    Ok(())
}

fn validate_ulid(field: &'static str, value: &str) -> Result<(), ClaimValidationError> {
    if !is_valid_ulid(value) {
        return Err(ClaimValidationError::InvalidUlid(field));
    }
    Ok(())
}

fn is_valid_ulid(value: &str) -> bool {
    value.len() == 26 && value.bytes().all(|byte| {
        byte.is_ascii_digit()
            || matches!(byte, b'A'..=b'H' | b'J'..=b'K' | b'M'..=b'N' | b'P'..=b'T' | b'V'..=b'Z')
    }) && Ulid::from_string(value).is_ok()
}
