use crate::err::{ErrorResponse, HttpError};

use super::{
    DEFAULT_EMAIL_OTP_LENGTH, DEFAULT_EMAIL_OTP_MAX_ATTEMPTS, DEFAULT_EMAIL_OTP_TTL_SECS,
    MFA_CHALLENGE_KEY_PREFIX, MFA_PENDING_LOGIN_KEY_PREFIX, MFA_VERIFIED_KEY_PREFIX,
    PASSWORDLESS_EMAIL_KEY_PREFIX,
};

pub(crate) fn normalize_email(value: &str) -> Result<String, ErrorResponse> {
    crate::etc::input::trim_required(value, "email")
        .map(str::to_string)
        .map_err(input_bad_request)
}

pub(crate) fn validate_code(code: &str) -> Result<(), ErrorResponse> {
    crate::etc::input::trim_required(code, "code")
        .map(|_| ())
        .map_err(input_bad_request)
}

pub(crate) fn normalize_purpose(
    value: Option<&str>,
    default: &str,
) -> Result<String, ErrorResponse> {
    let purpose = crate::etc::input::trim_required(value.unwrap_or(default), "purpose")
        .map_err(input_bad_request)?;
    crate::etc::input::enforce_max_len(purpose, 128, "purpose").map_err(input_bad_request)?;
    crate::etc::input::reject_control_chars(purpose, "purpose").map_err(input_bad_request)?;

    Ok(purpose.to_string())
}

pub(crate) fn validate_challenge_id(challenge_id: &str) -> Result<(), ErrorResponse> {
    if !crate::etc::input::is_fixed_len_ascii_hex(challenge_id, 32) {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "challengeId is invalid".to_string(),
        )));
    }

    Ok(())
}

pub(crate) fn email_otp_config() -> Result<::otp::MessageOtpConfig, ErrorResponse> {
    ::otp::MessageOtpConfig::numeric(
        otp_env("EMAIL_OTP_LENGTH", DEFAULT_EMAIL_OTP_LENGTH)?,
        otp_env("EMAIL_OTP_TTL_SECS", DEFAULT_EMAIL_OTP_TTL_SECS)?,
        otp_env("EMAIL_OTP_MAX_ATTEMPTS", DEFAULT_EMAIL_OTP_MAX_ATTEMPTS)?,
    )
    .map_err(email_otp_internal)
}

pub(crate) fn email_otp_pepper() -> Result<String, ErrorResponse> {
    let value = std::env::var("EMAIL_OTP_PEPPER").map_err(|_| {
        tracing::error!("EMAIL_OTP_PEPPER is not configured");
        ErrorResponse::from(HttpError::ServiceUnavailable(
            "email OTP temporarily unavailable".to_string(),
        ))
    })?;

    if value.trim().is_empty() {
        tracing::error!("EMAIL_OTP_PEPPER is empty");
        return Err(ErrorResponse::from(HttpError::ServiceUnavailable(
            "email OTP temporarily unavailable".to_string(),
        )));
    }

    Ok(value)
}

pub(crate) fn otp_env<T>(name: &str, default: T) -> Result<T, ErrorResponse>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    crate::etc::env::parse_or(name, default).map_err(|error| {
        tracing::error!(%error, "invalid OTP environment variable");
        ErrorResponse::from(HttpError::ServiceUnavailable(
            "OTP temporarily unavailable".to_string(),
        ))
    })
}

pub(crate) fn current_unix_time() -> Result<u64, ErrorResponse> {
    crate::etc::time::unix_now()
        .map_err(|_| ErrorResponse::internal("system time is before Unix epoch"))
}

pub(crate) fn ttl_until(expires_at: u64, now: u64) -> u64 {
    crate::etc::time::ttl_until(expires_at, now)
}

pub(crate) fn passwordless_email_key(challenge_id: &str) -> String {
    format!("{PASSWORDLESS_EMAIL_KEY_PREFIX}:{challenge_id}")
}

pub(crate) fn pending_login_key(challenge_id: &str) -> String {
    format!("{MFA_PENDING_LOGIN_KEY_PREFIX}:{challenge_id}")
}

pub(crate) fn mfa_challenge_key(user_id: &str, challenge_id: &str) -> String {
    format!("{MFA_CHALLENGE_KEY_PREFIX}:{user_id}:{challenge_id}")
}

pub(crate) fn mfa_verified_key(sid: &str, purpose: &str) -> String {
    format!("{MFA_VERIFIED_KEY_PREFIX}:{sid}:{purpose}")
}

pub(crate) fn random_challenge_id() -> Result<String, ErrorResponse> {
    let bytes = ::otp::generate_secret(16).map_err(email_otp_internal)?;
    Ok(hex_encode(&bytes))
}

pub(crate) fn render_email_body(body: &str) -> String {
    format!(
        "<!doctype html><html><body><p>{}</p></body></html>",
        escape_html(body)
    )
}

pub(crate) fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }

    escaped
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(crate) fn invalid_or_expired_code() -> ErrorResponse {
    ErrorResponse::from(HttpError::Unauthorized(
        "Invalid or expired code".to_string(),
    ))
}

pub(crate) fn verification_error(result: ::otp::MessageOtpVerification) -> ErrorResponse {
    match result {
        ::otp::MessageOtpVerification::Valid => {
            ErrorResponse::internal("unexpected valid OTP state")
        }
        ::otp::MessageOtpVerification::Invalid { attempts_remaining } => {
            ErrorResponse::from(HttpError::Unauthorized(format!(
                "Invalid code. {attempts_remaining} attempt(s) remaining"
            )))
        }
        ::otp::MessageOtpVerification::Expired | ::otp::MessageOtpVerification::AlreadyUsed => {
            invalid_or_expired_code()
        }
        ::otp::MessageOtpVerification::AttemptsExceeded => ErrorResponse::from(
            HttpError::TooManyRequests("Too many invalid OTP attempts".to_string()),
        ),
    }
}

pub(crate) fn email_otp_internal(error: impl std::fmt::Display) -> ErrorResponse {
    tracing::error!("email OTP error: {}", error);
    ErrorResponse::from(HttpError::ServiceUnavailable(
        "email OTP temporarily unavailable".to_string(),
    ))
}

pub(crate) fn otp_unavailable(error: store::StoreError) -> ErrorResponse {
    tracing::error!("OTP request guard unavailable: {}", error);
    ErrorResponse::from(HttpError::ServiceUnavailable(
        "OTP temporarily unavailable".to_string(),
    ))
}

fn input_bad_request(error: crate::etc::input::InputError) -> ErrorResponse {
    ErrorResponse::from(HttpError::BadRequest(error.to_string()))
}
