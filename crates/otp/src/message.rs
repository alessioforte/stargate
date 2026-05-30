use crate::crypto::{constant_time_eq, fill_random, hmac_sha256, random_index};
use crate::error::{OtpError, Result};
use serde::{Deserialize, Serialize};

const DEFAULT_NUMERIC_ALPHABET: &str = "0123456789";
const DEFAULT_ALPHANUMERIC_ALPHABET: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOtpKind {
    Email,
    Sms,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeAlphabet {
    symbols: Vec<char>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageOtpConfig {
    pub length: usize,
    pub ttl_seconds: u64,
    pub max_attempts: u8,
    pub alphabet: CodeAlphabet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssuedMessageOtp {
    pub code: String,
    pub record: MessageOtpRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageOtpRecord {
    pub id: [u8; 16],
    pub kind: MessageOtpKind,
    pub recipient: String,
    pub purpose: String,
    pub salt: [u8; 16],
    pub code_hash: [u8; 32],
    pub issued_at_unix: u64,
    pub expires_at_unix: u64,
    pub attempts_used: u8,
    pub max_attempts: u8,
    pub consumed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryContent {
    pub recipient: String,
    pub subject: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOtpVerification {
    Valid,
    Invalid { attempts_remaining: u8 },
    Expired,
    AttemptsExceeded,
    AlreadyUsed,
}

impl MessageOtpKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Sms => "sms",
        }
    }
}

impl CodeAlphabet {
    pub fn numeric() -> Self {
        Self::new(DEFAULT_NUMERIC_ALPHABET).expect("default numeric alphabet is valid")
    }

    pub fn upper_alphanumeric() -> Self {
        Self::new(DEFAULT_ALPHANUMERIC_ALPHABET).expect("default alphanumeric alphabet is valid")
    }

    pub fn new(symbols: &str) -> Result<Self> {
        let mut unique = Vec::new();

        for ch in symbols.chars() {
            if !ch.is_ascii() || ch.is_ascii_whitespace() {
                return Err(OtpError::InvalidAlphabet);
            }

            if !unique.contains(&ch) {
                unique.push(ch);
            }
        }

        if unique.len() < 2 {
            return Err(OtpError::InvalidAlphabet);
        }

        Ok(Self { symbols: unique })
    }

    pub fn symbols(&self) -> &[char] {
        &self.symbols
    }

    pub fn contains_all(&self, code: &str) -> bool {
        code.chars().all(|ch| self.symbols.contains(&ch))
    }

    fn generate(&self, length: usize) -> String {
        let mut code = String::with_capacity(length);

        for _ in 0..length {
            code.push(self.symbols[random_index(self.symbols.len())]);
        }

        code
    }
}

impl Default for MessageOtpConfig {
    fn default() -> Self {
        Self::numeric(6, 300, 5).expect("default message OTP config is valid")
    }
}

impl MessageOtpConfig {
    pub fn numeric(length: usize, ttl_seconds: u64, max_attempts: u8) -> Result<Self> {
        Self::new(length, ttl_seconds, max_attempts, CodeAlphabet::numeric())
    }

    pub fn alphanumeric(length: usize, ttl_seconds: u64, max_attempts: u8) -> Result<Self> {
        Self::new(
            length,
            ttl_seconds,
            max_attempts,
            CodeAlphabet::upper_alphanumeric(),
        )
    }

    pub fn new(
        length: usize,
        ttl_seconds: u64,
        max_attempts: u8,
        alphabet: CodeAlphabet,
    ) -> Result<Self> {
        if !(4..=32).contains(&length) {
            return Err(OtpError::InvalidOtpLength);
        }

        if ttl_seconds == 0 {
            return Err(OtpError::InvalidPeriod);
        }

        if max_attempts == 0 {
            return Err(OtpError::InvalidOtpLength);
        }

        Ok(Self {
            length,
            ttl_seconds,
            max_attempts,
            alphabet,
        })
    }
}

impl IssuedMessageOtp {
    pub fn delivery_content(&self, issuer: &str) -> DeliveryContent {
        match self.record.kind {
            MessageOtpKind::Email => DeliveryContent {
                recipient: self.record.recipient.clone(),
                subject: Some(format!("{issuer} verification code")),
                body: format!(
                    "Your {issuer} verification code is {}. It expires in {} minutes.",
                    self.code,
                    self.record.ttl_seconds().div_ceil(60)
                ),
            },
            MessageOtpKind::Sms => DeliveryContent {
                recipient: self.record.recipient.clone(),
                subject: None,
                body: format!(
                    "{issuer} code: {}. Expires in {} minutes.",
                    self.code,
                    self.record.ttl_seconds().div_ceil(60)
                ),
            },
        }
    }
}

impl MessageOtpRecord {
    pub fn verify(&mut self, code: &str, pepper: &[u8], now_unix: u64) -> MessageOtpVerification {
        if self.consumed {
            return MessageOtpVerification::AlreadyUsed;
        }

        if now_unix >= self.expires_at_unix {
            return MessageOtpVerification::Expired;
        }

        if self.attempts_used >= self.max_attempts {
            return MessageOtpVerification::AttemptsExceeded;
        }

        if pepper.is_empty() {
            self.attempts_used = self.attempts_used.saturating_add(1);
            return self.invalid_result();
        }

        let candidate = normalize_message_code(code);
        let candidate_hash = hash_message_otp(
            pepper,
            &self.id,
            self.kind,
            &self.recipient,
            &self.purpose,
            &self.salt,
            &candidate,
        );

        if constant_time_eq(&candidate_hash, &self.code_hash) {
            self.consumed = true;
            return MessageOtpVerification::Valid;
        }

        self.attempts_used = self.attempts_used.saturating_add(1);
        self.invalid_result()
    }

    pub fn is_expired(&self, now_unix: u64) -> bool {
        now_unix >= self.expires_at_unix
    }

    pub fn attempts_remaining(&self) -> u8 {
        self.max_attempts.saturating_sub(self.attempts_used)
    }

    pub fn ttl_seconds(&self) -> u64 {
        self.expires_at_unix.saturating_sub(self.issued_at_unix)
    }

    pub fn id_hex(&self) -> String {
        hex_encode(&self.id)
    }

    pub fn salt_hex(&self) -> String {
        hex_encode(&self.salt)
    }

    pub fn code_hash_hex(&self) -> String {
        hex_encode(&self.code_hash)
    }

    fn invalid_result(&self) -> MessageOtpVerification {
        if self.attempts_used >= self.max_attempts {
            MessageOtpVerification::AttemptsExceeded
        } else {
            MessageOtpVerification::Invalid {
                attempts_remaining: self.attempts_remaining(),
            }
        }
    }
}

impl MessageOtpVerification {
    pub const fn is_valid(self) -> bool {
        matches!(self, Self::Valid)
    }
}

pub fn issue_email_otp(
    recipient: impl Into<String>,
    purpose: impl Into<String>,
    pepper: &[u8],
    now_unix: u64,
    config: MessageOtpConfig,
) -> Result<IssuedMessageOtp> {
    issue_message_otp(
        MessageOtpKind::Email,
        recipient,
        purpose,
        pepper,
        now_unix,
        config,
    )
}

pub fn issue_sms_otp(
    recipient: impl Into<String>,
    purpose: impl Into<String>,
    pepper: &[u8],
    now_unix: u64,
    config: MessageOtpConfig,
) -> Result<IssuedMessageOtp> {
    issue_message_otp(
        MessageOtpKind::Sms,
        recipient,
        purpose,
        pepper,
        now_unix,
        config,
    )
}

pub fn issue_message_otp(
    kind: MessageOtpKind,
    recipient: impl Into<String>,
    purpose: impl Into<String>,
    pepper: &[u8],
    now_unix: u64,
    config: MessageOtpConfig,
) -> Result<IssuedMessageOtp> {
    if pepper.is_empty() {
        return Err(OtpError::EmptyPepper);
    }

    let recipient = recipient.into();
    if recipient.trim().is_empty() {
        return Err(OtpError::InvalidRecipient);
    }

    let purpose = purpose.into();
    if purpose.trim().is_empty() {
        return Err(OtpError::InvalidPurpose);
    }

    let expires_at_unix = now_unix
        .checked_add(config.ttl_seconds)
        .ok_or(OtpError::ExpiryOverflow)?;
    let code = config.alphabet.generate(config.length);
    let mut id = [0; 16];
    let mut salt = [0; 16];
    fill_random(&mut id);
    fill_random(&mut salt);

    let code_hash = hash_message_otp(pepper, &id, kind, &recipient, &purpose, &salt, &code);

    Ok(IssuedMessageOtp {
        code,
        record: MessageOtpRecord {
            id,
            kind,
            recipient,
            purpose,
            salt,
            code_hash,
            issued_at_unix: now_unix,
            expires_at_unix,
            attempts_used: 0,
            max_attempts: config.max_attempts,
            consumed: false,
        },
    })
}

fn hash_message_otp(
    pepper: &[u8],
    id: &[u8; 16],
    kind: MessageOtpKind,
    recipient: &str,
    purpose: &str,
    salt: &[u8; 16],
    code: &str,
) -> [u8; 32] {
    hmac_sha256(
        pepper,
        &[
            b"stargate:otp:message:v1",
            &[0],
            id,
            &[0],
            kind.as_str().as_bytes(),
            &[0],
            recipient.as_bytes(),
            &[0],
            purpose.as_bytes(),
            &[0],
            salt,
            &[0],
            code.as_bytes(),
        ],
    )
}

fn normalize_message_code(code: &str) -> String {
    code.chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEPPER: &[u8] = b"server-side-message-otp-pepper";

    #[test]
    fn issues_and_verifies_email_otp_once() {
        let issued = issue_email_otp(
            "alice@example.com",
            "login",
            PEPPER,
            1_700_000_000,
            MessageOtpConfig::default(),
        )
        .unwrap();
        let mut record = issued.record;

        assert_eq!(issued.code.len(), 6);
        assert_eq!(record.kind, MessageOtpKind::Email);
        assert!(record.code_hash_hex().len() == 64);
        assert_eq!(
            record.verify(
                &format!("{} {}", &issued.code[..3], &issued.code[3..]),
                PEPPER,
                1_700_000_100
            ),
            MessageOtpVerification::Valid
        );
        assert_eq!(
            record.verify(&issued.code, PEPPER, 1_700_000_100),
            MessageOtpVerification::AlreadyUsed
        );
    }

    #[test]
    fn rejects_invalid_codes_and_tracks_attempts() {
        let issued = issue_sms_otp(
            "+15555550100",
            "step-up",
            PEPPER,
            1_700_000_000,
            MessageOtpConfig::numeric(6, 300, 2).unwrap(),
        )
        .unwrap();
        let mut record = issued.record;

        assert_eq!(
            record.verify("000000", PEPPER, 1_700_000_010),
            MessageOtpVerification::Invalid {
                attempts_remaining: 1
            }
        );
        assert_eq!(
            record.verify("111111", PEPPER, 1_700_000_011),
            MessageOtpVerification::AttemptsExceeded
        );
        assert_eq!(
            record.verify(&issued.code, PEPPER, 1_700_000_012),
            MessageOtpVerification::AttemptsExceeded
        );
    }

    #[test]
    fn rejects_expired_message_otp() {
        let issued = issue_email_otp(
            "alice@example.com",
            "login",
            PEPPER,
            1_700_000_000,
            MessageOtpConfig::numeric(6, 30, 3).unwrap(),
        )
        .unwrap();
        let mut record = issued.record;

        assert_eq!(
            record.verify(&issued.code, PEPPER, 1_700_000_030),
            MessageOtpVerification::Expired
        );
    }

    #[test]
    fn renders_delivery_content() {
        let issued = issue_sms_otp(
            "+15555550100",
            "login",
            PEPPER,
            1_700_000_000,
            MessageOtpConfig::numeric(6, 61, 3).unwrap(),
        )
        .unwrap();

        let content = issued.delivery_content("Stargate");
        assert_eq!(content.recipient, "+15555550100");
        assert_eq!(content.subject, None);
        assert!(content.body.contains(&issued.code));
        assert!(content.body.contains("2 minutes"));
    }
}
