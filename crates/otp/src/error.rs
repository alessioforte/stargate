use std::fmt;

pub type Result<T> = std::result::Result<T, OtpError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OtpError {
    EmptySecret,
    EmptyPepper,
    InvalidAlgorithm,
    InvalidCode,
    InvalidCounter,
    InvalidDigits,
    InvalidPeriod,
    InvalidSecretLength,
    InvalidBase32Secret,
    InvalidIssuer,
    InvalidAccountName,
    InvalidRecipient,
    InvalidPurpose,
    InvalidAlphabet,
    InvalidOtpLength,
    ExpiryOverflow,
    TimeBeforeUnixEpoch,
}

impl fmt::Display for OtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySecret => f.write_str("OTP secret must not be empty"),
            Self::EmptyPepper => f.write_str("message OTP pepper must not be empty"),
            Self::InvalidAlgorithm => f.write_str("unsupported OTP hash algorithm"),
            Self::InvalidCode => f.write_str("OTP code is malformed"),
            Self::InvalidCounter => f.write_str("OTP counter is invalid"),
            Self::InvalidDigits => f.write_str("OTP digits must be between 6 and 10"),
            Self::InvalidPeriod => f.write_str("TOTP period must be greater than zero"),
            Self::InvalidSecretLength => f.write_str("OTP secret length is invalid"),
            Self::InvalidBase32Secret => f.write_str("Base32 OTP secret is invalid"),
            Self::InvalidIssuer => f.write_str("OTP issuer must not be empty"),
            Self::InvalidAccountName => f.write_str("OTP account name must not be empty"),
            Self::InvalidRecipient => f.write_str("OTP recipient must not be empty"),
            Self::InvalidPurpose => f.write_str("OTP purpose must not be empty"),
            Self::InvalidAlphabet => {
                f.write_str("OTP alphabet must contain at least two unique ASCII characters")
            }
            Self::InvalidOtpLength => f.write_str("message OTP length is invalid"),
            Self::ExpiryOverflow => f.write_str("message OTP expiry overflows u64"),
            Self::TimeBeforeUnixEpoch => f.write_str("system time is before the Unix epoch"),
        }
    }
}

impl std::error::Error for OtpError {}
