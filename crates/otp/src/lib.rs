//! One-time password primitives for Stargate.
//!
//! The crate covers HOTP (RFC 4226), TOTP (RFC 6238), authenticator-app
//! provisioning URIs, and short-lived email/SMS OTP challenges. It intentionally
//! does not send email or SMS; callers can persist the issued records and deliver
//! the returned code through the appropriate transport.

mod base32;
mod crypto;
mod error;
mod hotp;
mod message;
mod totp;
mod uri;

pub use base32::{decode_base32_secret, encode_base32_secret};
pub use crypto::HashAlgorithm;
pub use error::{OtpError, Result};
pub use hotp::{Hotp, HotpMatch};
pub use message::{
    CodeAlphabet, DeliveryContent, IssuedMessageOtp, MessageOtpConfig, MessageOtpKind,
    MessageOtpRecord, MessageOtpVerification, issue_email_otp, issue_message_otp, issue_sms_otp,
};
pub use totp::{Totp, TotpMatch, VerificationWindow};

pub const DEFAULT_SECRET_BYTES: usize = 20;

/// Generate a random binary secret suitable for HOTP/TOTP.
///
/// Authenticator apps commonly use 160-bit SHA-1 secrets, which corresponds to
/// the default 20-byte length.
pub fn generate_secret(length: usize) -> Result<Vec<u8>> {
    if length == 0 {
        return Err(OtpError::InvalidSecretLength);
    }

    let mut secret = vec![0; length];
    crypto::fill_random(&mut secret);
    Ok(secret)
}

/// Generate a random Base32 secret suitable for provisioning into authenticator apps.
pub fn generate_base32_secret(length: usize) -> Result<String> {
    Ok(encode_base32_secret(&generate_secret(length)?))
}

/// Generate a random default-length Base32 secret suitable for TOTP.
pub fn generate_default_base32_secret() -> String {
    encode_base32_secret(&generate_secret(DEFAULT_SECRET_BYTES).expect("positive secret length"))
}
