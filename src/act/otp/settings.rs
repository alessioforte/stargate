//! MFA and email OTP settings, read and validated once at startup like
//! `etc::features`: an invalid value, or a configuration that cannot work,
//! stops startup instead of failing sign-ins one request at a time.

use super::types::{MfaMode, OtpMethod};
use super::{
    DEFAULT_EMAIL_OTP_LENGTH, DEFAULT_EMAIL_OTP_MAX_ATTEMPTS, DEFAULT_EMAIL_OTP_TTL_SECS,
    DEFAULT_MFA_STEP_UP_TTL_SECS,
};
use crate::etc::env::{EnvError, parse_bool};
use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OtpSettings {
    pub(crate) mfa_mode: MfaMode,
    /// Methods MFA may use (`MFA_METHODS`); empty with `MFA_EMAIL_ENABLED=false`.
    pub(crate) mfa_methods: Vec<OtpMethod>,
    pub(crate) mfa_default_method: OtpMethod,
    /// Super-admins need MFA in `optional` mode.
    pub(crate) mfa_required_for_super_admin: bool,
    /// Super-admin sessions need an email step-up before admin grants.
    pub(crate) admin_step_up_required: bool,
    pub(crate) step_up_ttl_secs: u64,
    pub(crate) email_otp: ::otp::MessageOtpConfig,
    /// HMAC key for the codes (`EMAIL_OTP_PEPPER`); codes cannot be issued
    /// without it.
    pub(crate) email_otp_pepper: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum OtpSettingsError {
    #[error(transparent)]
    Env(#[from] EnvError),
    #[error("{0} needs an MFA method, but `MFA_EMAIL_ENABLED=false` leaves none")]
    NoMethod(&'static str),
    #[error("`MFA_ADMIN_STEP_UP_REQUIRED=true` needs MFA, but `MFA_MODE=off`")]
    StepUpWithoutMfa,
    #[error("`EMAIL_OTP_PEPPER` is required with {0}: it secures the emailed codes")]
    MissingPepper(&'static str),
}

impl OtpSettings {
    pub(crate) fn from_env() -> Result<Self, OtpSettingsError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, OtpSettingsError> {
        let var = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let invalid = |name: &str, value: String, message: String| EnvError::Parse {
            name: name.to_string(),
            value,
            message,
        };
        let flag = |name: &str, default: bool| match var(name) {
            None => Ok(default),
            Some(value) => parse_bool(&value)
                .ok_or_else(|| invalid(name, value, "expected true or false".to_string())),
        };
        let number = |name: &str, default: u64, min: u64, max: u64| match var(name) {
            None => Ok(default),
            Some(value) => match u64::from_str(&value) {
                Ok(number) if (min..=max).contains(&number) => Ok(number),
                _ => Err(invalid(
                    name,
                    value,
                    format!("expected a whole number from {min} to {max}"),
                )),
            },
        };

        let mfa_mode = match var("MFA_MODE") {
            None => MfaMode::Optional,
            Some(value) => match value.to_ascii_lowercase().as_str() {
                "off" => MfaMode::Off,
                "optional" => MfaMode::Optional,
                "required" => MfaMode::Required,
                _ => {
                    return Err(invalid(
                        "MFA_MODE",
                        value,
                        "expected off, optional, or required".to_string(),
                    )
                    .into());
                }
            },
        };
        let configured_methods = match var("MFA_METHODS") {
            None => vec![OtpMethod::Email],
            Some(value) => {
                parse_methods(&value).map_err(|message| invalid("MFA_METHODS", value, message))?
            }
        };
        let mfa_methods = if flag("MFA_EMAIL_ENABLED", true)? {
            configured_methods.clone()
        } else {
            Vec::new()
        };
        let mfa_default_method = match var("MFA_DEFAULT_METHOD") {
            None => configured_methods[0],
            Some(value) => {
                let method = parse_method(&value)
                    .map_err(|message| invalid("MFA_DEFAULT_METHOD", value.clone(), message))?;
                if !configured_methods.contains(&method) {
                    return Err(invalid(
                        "MFA_DEFAULT_METHOD",
                        value,
                        "it is not one of MFA_METHODS".to_string(),
                    )
                    .into());
                }
                method
            }
        };
        let mfa_required_for_super_admin = flag("MFA_REQUIRED_FOR_SUPER_ADMIN", false)?;
        let admin_step_up_required = flag("MFA_ADMIN_STEP_UP_REQUIRED", false)?;
        let step_up_ttl_secs = number(
            "MFA_STEP_UP_TTL_SECS",
            DEFAULT_MFA_STEP_UP_TTL_SECS,
            1,
            u64::MAX,
        )?;
        let length = number("EMAIL_OTP_LENGTH", DEFAULT_EMAIL_OTP_LENGTH as u64, 4, 32)? as usize;
        let ttl = number(
            "EMAIL_OTP_TTL_SECS",
            DEFAULT_EMAIL_OTP_TTL_SECS,
            1,
            u64::MAX,
        )?;
        let max_attempts = number(
            "EMAIL_OTP_MAX_ATTEMPTS",
            u64::from(DEFAULT_EMAIL_OTP_MAX_ATTEMPTS),
            1,
            u64::from(u8::MAX),
        )? as u8;
        let email_otp = ::otp::MessageOtpConfig::numeric(length, ttl, max_attempts)
            .expect("email OTP settings are within the checked bounds");
        let email_otp_pepper = var("EMAIL_OTP_PEPPER");

        // Configurations that make MFA mandatory for someone must be able to
        // deliver it, or those sign-ins always fail.
        let mandatory = match mfa_mode {
            MfaMode::Required => Some("`MFA_MODE=required`"),
            MfaMode::Optional if mfa_required_for_super_admin => {
                Some("`MFA_REQUIRED_FOR_SUPER_ADMIN=true`")
            }
            _ => None,
        };
        if let Some(reason) = mandatory {
            if mfa_methods.is_empty() {
                return Err(OtpSettingsError::NoMethod(reason));
            }
            if email_otp_pepper.is_none() {
                return Err(OtpSettingsError::MissingPepper(reason));
            }
        }
        if admin_step_up_required {
            if mfa_mode == MfaMode::Off {
                return Err(OtpSettingsError::StepUpWithoutMfa);
            }
            if mfa_methods.is_empty() {
                return Err(OtpSettingsError::NoMethod(
                    "`MFA_ADMIN_STEP_UP_REQUIRED=true`",
                ));
            }
            if email_otp_pepper.is_none() {
                return Err(OtpSettingsError::MissingPepper(
                    "`MFA_ADMIN_STEP_UP_REQUIRED=true`",
                ));
            }
        }

        Ok(Self {
            mfa_mode,
            mfa_methods,
            mfa_default_method,
            mfa_required_for_super_admin,
            admin_step_up_required,
            step_up_ttl_secs,
            email_otp,
            email_otp_pepper,
        })
    }

    /// Users can turn MFA on, but the codes cannot be issued.
    fn optional_mfa_without_pepper(&self) -> bool {
        self.mfa_mode == MfaMode::Optional
            && !self.mfa_methods.is_empty()
            && self.email_otp_pepper.is_none()
    }
}

/// An MFA method from configuration: only `email` is implemented.
fn parse_method(value: &str) -> Result<OtpMethod, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "email" => Ok(OtpMethod::Email),
        "sms" | "totp" => Err(format!(
            "`{}` is not implemented yet (only email)",
            value.trim()
        )),
        _ => Err(format!("`{}` is not an MFA method (email)", value.trim())),
    }
}

fn parse_methods(value: &str) -> Result<Vec<OtpMethod>, String> {
    let mut methods = Vec::new();
    for entry in value.split(',').filter(|entry| !entry.trim().is_empty()) {
        let method = parse_method(entry)?;
        if !methods.contains(&method) {
            methods.push(method);
        }
    }
    if methods.is_empty() {
        return Err("no method listed".to_string());
    }
    Ok(methods)
}

impl fmt::Display for OtpSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let methods: Vec<&str> = self
            .mfa_methods
            .iter()
            .map(|method| match method {
                OtpMethod::Email => "email",
            })
            .collect();
        let mode = match self.mfa_mode {
            MfaMode::Off => "off",
            MfaMode::Optional => "optional",
            MfaMode::Required => "required",
        };
        write!(
            f,
            "mode={mode} methods=[{}] super_admin_required={} admin_step_up={} step_up_ttl={}s email_otp(length={} ttl={}s attempts={} pepper={})",
            methods.join(", "),
            self.mfa_required_for_super_admin,
            self.admin_step_up_required,
            self.step_up_ttl_secs,
            self.email_otp.length,
            self.email_otp.ttl_seconds,
            self.email_otp.max_attempts,
            if self.email_otp_pepper.is_some() {
                "set"
            } else {
                "unset"
            },
        )
    }
}

static SETTINGS: OnceLock<OtpSettings> = OnceLock::new();

/// Reads and validates the settings; the server calls it once at startup.
pub(crate) fn init() -> Result<&'static OtpSettings, OtpSettingsError> {
    let settings = OtpSettings::from_env()?;
    let settings = SETTINGS.get_or_init(|| settings);
    tracing::info!("MFA: {settings}");
    if settings.optional_mfa_without_pepper() {
        tracing::warn!(
            "EMAIL_OTP_PEPPER is unset: users who turn MFA on cannot sign in until it is set"
        );
    }
    Ok(settings)
}

/// The settings in effect. Outside the server (tests, CLI commands) they are
/// read on first use.
pub(crate) fn otp_settings() -> &'static OtpSettings {
    SETTINGS.get_or_init(|| OtpSettings::from_env().expect("MFA settings are validated at startup"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn settings_from(vars: &[(&str, &str)]) -> Result<OtpSettings, OtpSettingsError> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        OtpSettings::from_lookup(|name| vars.get(name).cloned())
    }

    fn message(vars: &[(&str, &str)]) -> String {
        settings_from(vars).unwrap_err().to_string()
    }

    #[test]
    fn defaults_are_optional_email_mfa() {
        let settings = settings_from(&[]).unwrap();
        assert_eq!(settings.mfa_mode, MfaMode::Optional);
        assert_eq!(settings.mfa_methods, [OtpMethod::Email]);
        assert_eq!(settings.mfa_default_method, OtpMethod::Email);
        assert!(!settings.mfa_required_for_super_admin);
        assert!(!settings.admin_step_up_required);
        assert_eq!(settings.step_up_ttl_secs, 300);
        assert_eq!(
            (
                settings.email_otp.length,
                settings.email_otp.ttl_seconds,
                settings.email_otp.max_attempts
            ),
            (6, 300, 5)
        );
        assert_eq!(settings.email_otp_pepper, None);
        assert!(settings.optional_mfa_without_pepper());
        assert_eq!(
            settings.to_string(),
            "mode=optional methods=[email] super_admin_required=false admin_step_up=false step_up_ttl=300s email_otp(length=6 ttl=300s attempts=5 pepper=unset)"
        );
    }

    #[test]
    fn values_are_parsed_strictly() {
        for (vars, name) in [
            (&[("MFA_MODE", "requried")][..], "MFA_MODE"),
            (&[("MFA_EMAIL_ENABLED", "flase")][..], "MFA_EMAIL_ENABLED"),
            (
                &[("MFA_REQUIRED_FOR_SUPER_ADMIN", "maybe")][..],
                "MFA_REQUIRED_FOR_SUPER_ADMIN",
            ),
            (
                &[("MFA_ADMIN_STEP_UP_REQUIRED", "y")][..],
                "MFA_ADMIN_STEP_UP_REQUIRED",
            ),
            (&[("MFA_METHODS", "emial")][..], "MFA_METHODS"),
            (&[("MFA_METHODS", "email,totp")][..], "MFA_METHODS"),
            (&[("MFA_METHODS", " , ")][..], "MFA_METHODS"),
            (&[("MFA_DEFAULT_METHOD", "sms")][..], "MFA_DEFAULT_METHOD"),
            (&[("MFA_STEP_UP_TTL_SECS", "0")][..], "MFA_STEP_UP_TTL_SECS"),
            (
                &[("MFA_STEP_UP_TTL_SECS", "5m")][..],
                "MFA_STEP_UP_TTL_SECS",
            ),
            (&[("EMAIL_OTP_LENGTH", "3")][..], "EMAIL_OTP_LENGTH"),
            (&[("EMAIL_OTP_LENGTH", "33")][..], "EMAIL_OTP_LENGTH"),
            (&[("EMAIL_OTP_TTL_SECS", "0")][..], "EMAIL_OTP_TTL_SECS"),
            (
                &[("EMAIL_OTP_MAX_ATTEMPTS", "0")][..],
                "EMAIL_OTP_MAX_ATTEMPTS",
            ),
            (
                &[("EMAIL_OTP_MAX_ATTEMPTS", "256")][..],
                "EMAIL_OTP_MAX_ATTEMPTS",
            ),
        ] {
            let error = message(vars);
            assert!(error.contains(name), "{vars:?}: {error}");
        }

        let settings = settings_from(&[
            ("MFA_MODE", " Required "),
            ("MFA_METHODS", "Email, email"),
            ("EMAIL_OTP_PEPPER", "pepper"),
            ("EMAIL_OTP_LENGTH", "8"),
            ("MFA_REQUIRED_FOR_SUPER_ADMIN", "off"),
        ])
        .unwrap();
        assert_eq!(settings.mfa_mode, MfaMode::Required);
        assert_eq!(settings.mfa_methods, [OtpMethod::Email]);
        assert_eq!(settings.email_otp.length, 8);
        assert!(!settings.optional_mfa_without_pepper());
    }

    #[test]
    fn mandatory_mfa_needs_a_method_and_the_pepper() {
        assert!(message(&[("MFA_MODE", "required")]).contains("`MFA_MODE=required`"));
        assert!(
            message(&[("MFA_REQUIRED_FOR_SUPER_ADMIN", "true")])
                .contains("`MFA_REQUIRED_FOR_SUPER_ADMIN=true`")
        );
        assert!(
            message(&[
                ("MFA_MODE", "required"),
                ("MFA_EMAIL_ENABLED", "false"),
                ("EMAIL_OTP_PEPPER", "pepper"),
            ])
            .contains("leaves none")
        );
        // Super-admin MFA only matters in optional mode.
        assert!(
            settings_from(&[
                ("MFA_MODE", "off"),
                ("MFA_REQUIRED_FOR_SUPER_ADMIN", "true")
            ])
            .is_ok()
        );
        assert!(
            settings_from(&[
                ("MFA_REQUIRED_FOR_SUPER_ADMIN", "true"),
                ("EMAIL_OTP_PEPPER", "pepper"),
            ])
            .is_ok()
        );
    }

    #[test]
    fn admin_step_up_needs_mfa() {
        assert!(matches!(
            settings_from(&[
                ("MFA_ADMIN_STEP_UP_REQUIRED", "true"),
                ("MFA_MODE", "off"),
                ("EMAIL_OTP_PEPPER", "pepper"),
            ])
            .unwrap_err(),
            OtpSettingsError::StepUpWithoutMfa
        ));
        assert!(message(&[("MFA_ADMIN_STEP_UP_REQUIRED", "true")]).contains("EMAIL_OTP_PEPPER"));
        assert!(
            settings_from(&[
                ("MFA_ADMIN_STEP_UP_REQUIRED", "true"),
                ("EMAIL_OTP_PEPPER", "pepper"),
            ])
            .unwrap()
            .admin_step_up_required
        );
    }

    #[test]
    fn mfa_off_or_disabled_email_needs_no_pepper() {
        for vars in [
            &[("MFA_MODE", "off")][..],
            &[("MFA_EMAIL_ENABLED", "false")][..],
        ] {
            let settings = settings_from(vars).unwrap();
            assert!(!settings.optional_mfa_without_pepper(), "{vars:?}");
        }
    }
}
