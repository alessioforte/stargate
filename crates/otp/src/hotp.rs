use crate::base32::{decode_base32_secret, encode_base32_secret};
use crate::crypto::{HashAlgorithm, constant_time_eq, hmac_digest};
use crate::error::{OtpError, Result};
use crate::uri::{percent_encode, validate_account_name, validate_issuer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotp {
    secret: Vec<u8>,
    digits: u32,
    algorithm: HashAlgorithm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotpMatch {
    pub counter: u64,
    pub next_counter: u64,
}

impl Hotp {
    pub fn new(secret: impl Into<Vec<u8>>) -> Result<Self> {
        Self::with_algorithm(secret, 6, HashAlgorithm::Sha1)
    }

    pub fn with_digits(secret: impl Into<Vec<u8>>, digits: u32) -> Result<Self> {
        Self::with_algorithm(secret, digits, HashAlgorithm::Sha1)
    }

    pub fn with_algorithm(
        secret: impl Into<Vec<u8>>,
        digits: u32,
        algorithm: HashAlgorithm,
    ) -> Result<Self> {
        let secret = secret.into();
        validate_secret(&secret)?;
        validate_digits(digits)?;

        Ok(Self {
            secret,
            digits,
            algorithm,
        })
    }

    pub fn from_base32_secret(secret: &str, digits: u32, algorithm: HashAlgorithm) -> Result<Self> {
        Self::with_algorithm(decode_base32_secret(secret)?, digits, algorithm)
    }

    pub fn secret(&self) -> &[u8] {
        &self.secret
    }

    pub fn base32_secret(&self) -> String {
        encode_base32_secret(&self.secret)
    }

    pub const fn digits(&self) -> u32 {
        self.digits
    }

    pub const fn algorithm(&self) -> HashAlgorithm {
        self.algorithm
    }

    pub fn generate(&self, counter: u64) -> String {
        let digest = hmac_digest(self.algorithm, &self.secret, &counter.to_be_bytes());
        let value = dynamic_truncate(&digest);
        format_code(value, self.digits)
    }

    pub fn verify(&self, code: &str, counter: u64) -> bool {
        let Ok(code) = normalize_numeric_code(code, self.digits) else {
            return false;
        };

        constant_time_eq(self.generate(counter).as_bytes(), code.as_bytes())
    }

    pub fn verify_look_ahead(
        &self,
        code: &str,
        counter: u64,
        look_ahead: u64,
    ) -> Option<HotpMatch> {
        let code = normalize_numeric_code(code, self.digits).ok()?;
        let end_counter = counter.checked_add(look_ahead)?;

        for candidate in counter..=end_counter {
            if constant_time_eq(self.generate(candidate).as_bytes(), code.as_bytes()) {
                return Some(HotpMatch {
                    counter: candidate,
                    next_counter: candidate.checked_add(1)?,
                });
            }
        }

        None
    }

    pub fn provisioning_uri(
        &self,
        issuer: &str,
        account_name: &str,
        counter: u64,
    ) -> Result<String> {
        validate_issuer(issuer)?;
        validate_account_name(account_name)?;

        Ok(format!(
            "otpauth://hotp/{}:{}?secret={}&issuer={}&algorithm={}&digits={}&counter={}",
            percent_encode(issuer),
            percent_encode(account_name),
            self.base32_secret(),
            percent_encode(issuer),
            self.algorithm.as_otpauth_value(),
            self.digits,
            counter
        ))
    }
}

pub(crate) fn validate_digits(digits: u32) -> Result<()> {
    if !(6..=10).contains(&digits) {
        return Err(OtpError::InvalidDigits);
    }

    Ok(())
}

pub(crate) fn normalize_numeric_code(code: &str, digits: u32) -> Result<String> {
    let mut normalized = String::with_capacity(code.len());

    for ch in code.chars() {
        if ch.is_ascii_whitespace() {
            continue;
        }

        if !ch.is_ascii_digit() {
            return Err(OtpError::InvalidCode);
        }

        normalized.push(ch);
    }

    if normalized.len() != digits as usize {
        return Err(OtpError::InvalidCode);
    }

    Ok(normalized)
}

fn validate_secret(secret: &[u8]) -> Result<()> {
    if secret.is_empty() {
        return Err(OtpError::EmptySecret);
    }

    Ok(())
}

fn dynamic_truncate(digest: &[u8]) -> u32 {
    let offset = usize::from(digest[digest.len() - 1] & 0x0f);
    let bytes = &digest[offset..offset + 4];

    ((u32::from(bytes[0]) & 0x7f) << 24)
        | (u32::from(bytes[1]) << 16)
        | (u32::from(bytes[2]) << 8)
        | u32::from(bytes[3])
}

fn format_code(value: u32, digits: u32) -> String {
    let modulo = 10u64.pow(digits);
    let code = u64::from(value) % modulo;
    format!("{code:0width$}", width = digits as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotp_matches_rfc4226_test_vectors() {
        let hotp = Hotp::with_digits(b"12345678901234567890".to_vec(), 6).unwrap();
        let expected = [
            "755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583",
            "399871", "520489",
        ];

        for (counter, code) in expected.into_iter().enumerate() {
            assert_eq!(hotp.generate(counter as u64), code);
            assert!(hotp.verify(code, counter as u64));
        }
    }

    #[test]
    fn hotp_verifies_with_look_ahead() {
        let hotp = Hotp::with_digits(b"12345678901234567890".to_vec(), 6).unwrap();
        let matched = hotp.verify_look_ahead("338314", 0, 5).unwrap();

        assert_eq!(matched.counter, 4);
        assert_eq!(matched.next_counter, 5);
        assert!(hotp.verify_look_ahead("338314", 0, 3).is_none());
    }

    #[test]
    fn hotp_builds_provisioning_uri() {
        let hotp = Hotp::with_digits(b"Hello!\xde\xad\xbe\xef".to_vec(), 6).unwrap();

        assert_eq!(
            hotp.provisioning_uri("Stargate IAM", "alice@example.com", 42)
                .unwrap(),
            "otpauth://hotp/Stargate%20IAM:alice%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=Stargate%20IAM&algorithm=SHA1&digits=6&counter=42"
        );
    }
}
