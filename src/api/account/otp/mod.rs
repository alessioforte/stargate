pub(crate) mod login_mfa;
pub(crate) mod mfa;
pub(crate) mod passwordless;

#[allow(unused_imports)]
pub use crate::act::otp::{
    LoginMfaRequiredResponse, MfaChallengeRequestBody, MfaChallengeVerifyRequestBody,
    MfaMethodsResponse, MfaMode, MfaVerificationResponse, OtpChallengeResponse, OtpMethod,
    PasswordlessEmailOtpRequestBody, PasswordlessEmailOtpVerifyRequestBody,
};
pub(crate) use login_mfa::maybe_start_login_mfa;
pub use login_mfa::put_login_mfa_challenge;
pub use mfa::{get_mfa_methods, post_mfa_challenge, put_mfa_challenge};
pub use passwordless::{post_login_email_otp, put_login_email_otp};

#[cfg(test)]
mod tests {
    use crate::act::otp::DEFAULT_MFA_PURPOSE;
    use crate::act::otp::types::OtpMethod;
    use crate::act::otp::util::{escape_html, normalize_purpose, ttl_until, validate_challenge_id};

    #[test]
    fn challenge_id_must_be_32_hex_chars() {
        assert!(validate_challenge_id("0123456789abcdef0123456789abcdef").is_ok());
        assert!(validate_challenge_id("0123456789abcdef").is_err());
        assert!(validate_challenge_id("0123456789abcdef0123456789abcdeg").is_err());
    }

    #[test]
    fn html_body_escapes_untrusted_text() {
        let rendered = format!(
            "<!doctype html><html><body><p>{}</p></body></html>",
            escape_html("Code <123> & \"quote\"")
        );

        assert!(rendered.contains("&lt;123&gt;"));
        assert!(rendered.contains("&amp;"));
        assert!(rendered.contains("&quot;quote&quot;"));
    }

    #[test]
    fn ttl_until_never_returns_zero() {
        assert_eq!(ttl_until(10, 20), 1);
        assert_eq!(ttl_until(20, 20), 1);
        assert_eq!(ttl_until(30, 20), 10);
    }

    #[test]
    fn purpose_rejects_control_characters() {
        assert!(normalize_purpose(Some("login"), DEFAULT_MFA_PURPOSE).is_ok());
        assert!(normalize_purpose(Some("admin\0step"), DEFAULT_MFA_PURPOSE).is_err());
        assert!(normalize_purpose(Some("admin\nstep"), DEFAULT_MFA_PURPOSE).is_err());
    }

    #[test]
    fn public_method_type_stays_email_first() {
        assert_eq!(format!("{:?}", OtpMethod::Email), "Email");
    }
}
