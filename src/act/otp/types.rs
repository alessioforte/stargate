use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OtpMethod {
    Email,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MfaMode {
    Off,
    Optional,
    Required,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordlessEmailOtpRequestBody {
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordlessEmailOtpVerifyRequestBody {
    pub challenge_id: String,
    pub code: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OtpChallengeResponse {
    pub challenge_id: String,
    pub method: OtpMethod,
    pub expires_at_unix: u64,
    pub ttl_seconds: u64,
    pub max_attempts: u8,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginMfaRequiredResponse {
    pub mfa_required: bool,
    pub challenge_id: String,
    pub method: OtpMethod,
    pub expires_at_unix: u64,
    pub ttl_seconds: u64,
    pub max_attempts: u8,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaMethodsResponse {
    pub mode: MfaMode,
    pub required: bool,
    pub methods: Vec<OtpMethod>,
    pub preferred_method: Option<OtpMethod>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaChallengeRequestBody {
    pub method: OtpMethod,
    pub purpose: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MfaChallengeVerifyRequestBody {
    pub code: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaVerificationResponse {
    pub method: OtpMethod,
    pub purpose: String,
    pub verified_at_unix: u64,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredOtpChallenge {
    pub(crate) user_id: String,
    pub(crate) method: OtpMethod,
    pub(crate) purpose: String,
    pub(crate) record: ::otp::MessageOtpRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PendingMfaLogin {
    pub(crate) user_id: String,
    pub(crate) method: OtpMethod,
    pub(crate) challenge: StoredOtpChallenge,
    pub(crate) auth_time: usize,
    /// Org explicitly requested at password login, honored when the session
    /// is finally issued after MFA verification. Serde-default so pending
    /// challenges created before this field deserialize.
    #[serde(default)]
    pub(crate) requested_org_id: Option<String>,
    pub(crate) created_at_unix: u64,
    pub(crate) expires_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MfaVerificationMarker {
    pub(crate) user_id: String,
    pub(crate) method: OtpMethod,
    pub(crate) purpose: String,
    pub(crate) verified_at_unix: u64,
    pub(crate) expires_at_unix: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthenticatedUser {
    pub(crate) user: db::ent::User,
    pub(crate) sid: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MfaPolicy {
    pub(crate) mode: MfaMode,
    pub(crate) required: bool,
    pub(crate) methods: Vec<OtpMethod>,
    pub(crate) preferred_method: Option<OtpMethod>,
}

#[derive(Debug)]
pub(crate) struct EmailOtpDelivery {
    pub(crate) recipient: String,
    pub(crate) name: Option<String>,
    pub(crate) subject: String,
    pub(crate) body: String,
}

pub(crate) enum VerifiedChallenge {
    Valid(StoredOtpChallenge),
    Rejected(::otp::MessageOtpVerification),
}

pub(crate) enum VerifiedPendingLogin {
    Valid(Box<PendingMfaLogin>),
    Rejected(::otp::MessageOtpVerification),
}
