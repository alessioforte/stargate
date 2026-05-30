use std::time::{SystemTime, UNIX_EPOCH};

use crate::base32::decode_base32_secret;
use crate::crypto::{HashAlgorithm, constant_time_eq};
use crate::error::{OtpError, Result};
use crate::hotp::{Hotp, normalize_numeric_code, validate_digits};
use crate::uri::{percent_encode, validate_account_name, validate_issuer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Totp {
    hotp: Hotp,
    period: u64,
    t0: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationWindow {
    pub past: u64,
    pub future: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TotpMatch {
    pub counter: u64,
    pub drift_steps: i64,
}

impl VerificationWindow {
    pub const fn exact() -> Self {
        Self { past: 0, future: 0 }
    }

    pub const fn symmetric(steps: u64) -> Self {
        Self {
            past: steps,
            future: steps,
        }
    }
}

impl Default for VerificationWindow {
    fn default() -> Self {
        Self::symmetric(1)
    }
}

impl Totp {
    pub fn new(secret: impl Into<Vec<u8>>) -> Result<Self> {
        Self::with_algorithm(secret, 6, 30, HashAlgorithm::Sha1)
    }

    pub fn with_period(secret: impl Into<Vec<u8>>, digits: u32, period: u64) -> Result<Self> {
        Self::with_algorithm(secret, digits, period, HashAlgorithm::Sha1)
    }

    pub fn with_algorithm(
        secret: impl Into<Vec<u8>>,
        digits: u32,
        period: u64,
        algorithm: HashAlgorithm,
    ) -> Result<Self> {
        Self::with_t0(secret, digits, period, algorithm, 0)
    }

    pub fn with_t0(
        secret: impl Into<Vec<u8>>,
        digits: u32,
        period: u64,
        algorithm: HashAlgorithm,
        t0: u64,
    ) -> Result<Self> {
        validate_digits(digits)?;
        if period == 0 {
            return Err(OtpError::InvalidPeriod);
        }

        Ok(Self {
            hotp: Hotp::with_algorithm(secret, digits, algorithm)?,
            period,
            t0,
        })
    }

    pub fn from_base32_secret(
        secret: &str,
        digits: u32,
        period: u64,
        algorithm: HashAlgorithm,
    ) -> Result<Self> {
        Self::with_algorithm(decode_base32_secret(secret)?, digits, period, algorithm)
    }

    pub fn secret(&self) -> &[u8] {
        self.hotp.secret()
    }

    pub fn base32_secret(&self) -> String {
        self.hotp.base32_secret()
    }

    pub const fn digits(&self) -> u32 {
        self.hotp.digits()
    }

    pub const fn algorithm(&self) -> HashAlgorithm {
        self.hotp.algorithm()
    }

    pub const fn period(&self) -> u64 {
        self.period
    }

    pub const fn t0(&self) -> u64 {
        self.t0
    }

    pub fn counter_at(&self, unix_time: u64) -> Result<u64> {
        if unix_time < self.t0 {
            return Err(OtpError::InvalidCounter);
        }

        Ok((unix_time - self.t0) / self.period)
    }

    pub fn generate_at(&self, unix_time: u64) -> Result<String> {
        Ok(self.hotp.generate(self.counter_at(unix_time)?))
    }

    pub fn generate_current(&self) -> Result<String> {
        self.generate_at(current_unix_time()?)
    }

    pub fn verify_at(
        &self,
        code: &str,
        unix_time: u64,
        window: VerificationWindow,
    ) -> Option<TotpMatch> {
        let code = normalize_numeric_code(code, self.digits()).ok()?;
        let counter = self.counter_at(unix_time).ok()?;
        let start = counter.saturating_sub(window.past);
        let end = counter.checked_add(window.future)?;

        for candidate in start..=end {
            if constant_time_eq(self.hotp.generate(candidate).as_bytes(), code.as_bytes()) {
                return Some(TotpMatch {
                    counter: candidate,
                    drift_steps: candidate as i64 - counter as i64,
                });
            }
        }

        None
    }

    pub fn verify_current(
        &self,
        code: &str,
        window: VerificationWindow,
    ) -> Result<Option<TotpMatch>> {
        Ok(self.verify_at(code, current_unix_time()?, window))
    }

    pub fn provisioning_uri(&self, issuer: &str, account_name: &str) -> Result<String> {
        validate_issuer(issuer)?;
        validate_account_name(account_name)?;

        Ok(format!(
            "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm={}&digits={}&period={}",
            percent_encode(issuer),
            percent_encode(account_name),
            self.base32_secret(),
            percent_encode(issuer),
            self.algorithm().as_otpauth_value(),
            self.digits(),
            self.period
        ))
    }
}

fn current_unix_time() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| OtpError::TimeBeforeUnixEpoch)?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totp_matches_rfc6238_sha1_vectors() {
        let totp =
            Totp::with_algorithm(b"12345678901234567890".to_vec(), 8, 30, HashAlgorithm::Sha1)
                .unwrap();

        assert_eq!(totp.generate_at(59).unwrap(), "94287082");
        assert_eq!(totp.generate_at(1_111_111_109).unwrap(), "07081804");
        assert_eq!(totp.generate_at(1_111_111_111).unwrap(), "14050471");
        assert_eq!(totp.generate_at(1_234_567_890).unwrap(), "89005924");
        assert_eq!(totp.generate_at(2_000_000_000).unwrap(), "69279037");
        assert_eq!(totp.generate_at(20_000_000_000).unwrap(), "65353130");
    }

    #[test]
    fn totp_matches_rfc6238_sha256_vectors() {
        let totp = Totp::with_algorithm(
            b"12345678901234567890123456789012".to_vec(),
            8,
            30,
            HashAlgorithm::Sha256,
        )
        .unwrap();

        assert_eq!(totp.generate_at(59).unwrap(), "46119246");
        assert_eq!(totp.generate_at(1_111_111_109).unwrap(), "68084774");
        assert_eq!(totp.generate_at(1_111_111_111).unwrap(), "67062674");
        assert_eq!(totp.generate_at(1_234_567_890).unwrap(), "91819424");
        assert_eq!(totp.generate_at(2_000_000_000).unwrap(), "90698825");
        assert_eq!(totp.generate_at(20_000_000_000).unwrap(), "77737706");
    }

    #[test]
    fn totp_matches_rfc6238_sha512_vectors() {
        let totp = Totp::with_algorithm(
            b"1234567890123456789012345678901234567890123456789012345678901234".to_vec(),
            8,
            30,
            HashAlgorithm::Sha512,
        )
        .unwrap();

        assert_eq!(totp.generate_at(59).unwrap(), "90693936");
        assert_eq!(totp.generate_at(1_111_111_109).unwrap(), "25091201");
        assert_eq!(totp.generate_at(1_111_111_111).unwrap(), "99943326");
        assert_eq!(totp.generate_at(1_234_567_890).unwrap(), "93441116");
        assert_eq!(totp.generate_at(2_000_000_000).unwrap(), "38618901");
        assert_eq!(totp.generate_at(20_000_000_000).unwrap(), "47863826");
    }

    #[test]
    fn totp_verifies_with_window() {
        let totp =
            Totp::with_algorithm(b"12345678901234567890".to_vec(), 8, 30, HashAlgorithm::Sha1)
                .unwrap();

        let matched = totp
            .verify_at("94287082", 89, VerificationWindow::symmetric(1))
            .unwrap();

        assert_eq!(matched.counter, 1);
        assert_eq!(matched.drift_steps, -1);
        assert!(
            totp.verify_at("94287082", 89, VerificationWindow::exact())
                .is_none()
        );
    }

    #[test]
    fn totp_builds_provisioning_uri() {
        let totp = Totp::with_algorithm(
            b"Hello!\xde\xad\xbe\xef".to_vec(),
            6,
            30,
            HashAlgorithm::Sha1,
        )
        .unwrap();

        assert_eq!(
            totp.provisioning_uri("Stargate IAM", "alice@example.com")
                .unwrap(),
            "otpauth://totp/Stargate%20IAM:alice%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=Stargate%20IAM&algorithm=SHA1&digits=6&period=30"
        );
    }
}
