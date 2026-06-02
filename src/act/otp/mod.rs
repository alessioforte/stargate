pub mod auth;
pub mod challenge;
pub mod delivery;
pub mod policy;
pub mod rate;
pub mod service;
pub mod types;
pub mod util;

pub(crate) const PASSWORDLESS_EMAIL_KEY_PREFIX: &str = "account:login:otp:email";
pub(crate) const MFA_PENDING_LOGIN_KEY_PREFIX: &str = "account:mfa:pending-login";
pub(crate) const MFA_CHALLENGE_KEY_PREFIX: &str = "account:mfa:challenge";
pub(crate) const MFA_VERIFIED_KEY_PREFIX: &str = "account:mfa:verified";
pub(crate) const OTP_REQUEST_KEY_PREFIX: &str = "account:otp:request";
pub(crate) const DEFAULT_EMAIL_OTP_LENGTH: usize = 6;
pub(crate) const DEFAULT_EMAIL_OTP_TTL_SECS: u64 = 300;
pub(crate) const DEFAULT_EMAIL_OTP_MAX_ATTEMPTS: u8 = 5;
pub(crate) const DEFAULT_PASSWORDLESS_PURPOSE: &str = "passwordless_login";
pub(crate) const DEFAULT_MFA_PURPOSE: &str = "login";
pub(crate) const ADMIN_STEP_UP_PURPOSE: &str = "admin";
pub(crate) const DEFAULT_MFA_STEP_UP_TTL_SECS: u64 = 300;
pub(crate) const OTP_REQUEST_LOCKOUT_SECS: u64 = 300;
pub(crate) const MAX_OTP_SOURCE_REQUESTS: i32 = 30;
pub(crate) const MAX_OTP_SUBJECT_REQUESTS: i32 = 5;
pub(crate) const ISSUER: &str = "Stargate";

pub(crate) use challenge::has_valid_mfa_verification;
pub(crate) use policy::admin_step_up_required;
pub use types::{
    LoginMfaRequiredResponse, MfaChallengeRequestBody, MfaChallengeVerifyRequestBody,
    MfaMethodsResponse, MfaMode, MfaVerificationResponse, OtpChallengeResponse, OtpMethod,
    PasswordlessEmailOtpRequestBody, PasswordlessEmailOtpVerifyRequestBody,
};
