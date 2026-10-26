//! Capabilities an operator can switch off, read once at startup.
//!
//! A disabled capability keeps its routes mounted and answers
//! `404 feature.disabled` (`api::middleware::features`): unmatched paths fall
//! through to the gateway, which could proxy them to an upstream.

use crate::conf::env::{EnvError, parse_bool};
use std::fmt;
use std::sync::OnceLock;

/// A capability that can be off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    /// Self-registration, `POST /signup` (`SIGNUP_MODE=open`).
    Signup,
    /// Admin invitations and completing pending signups with the emailed
    /// link (`SIGNUP_MODE` other than `closed`).
    Invitations,
    /// Signing in with an emailed code instead of a password,
    /// `/account/login/otp/email` (`PASSWORDLESS_LOGIN_ENABLED`).
    PasswordlessLogin,
    /// Serving the Console app (`CONSOLE_ENABLED`).
    Console,
    /// Social sign-in with any provider (`/oauth/state`).
    Social,
    SocialGoogle,
    SocialGithub,
}

impl Feature {
    /// The name reported in `feature.disabled` errors.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Signup => "signup",
            Self::Invitations => "invitations",
            Self::PasswordlessLogin => "passwordless_login",
            Self::Console => "console",
            Self::Social => "social",
            Self::SocialGoogle => "social_google",
            Self::SocialGithub => "social_github",
        }
    }
}

/// Who may create accounts (`SIGNUP_MODE`). Administrators can always create
/// them directly (`POST /admin/users`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SignupMode {
    /// Anyone: `POST /signup`, and the first social sign-in creates the
    /// account, for an allowed email domain.
    Open,
    /// Admin invitations only; social sign-in only for existing accounts.
    #[default]
    Invite,
    /// Administrators create accounts; no invitations.
    Closed,
}

impl SignupMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Invite => "invite",
            Self::Closed => "closed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "invite" => Some(Self::Invite),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

/// Why someone cannot create their own account.
#[derive(Debug, PartialEq, Eq)]
pub enum SelfServiceRefusal {
    /// `SIGNUP_MODE` is not `open`.
    Disabled,
    /// The email's domain is not in `SIGNUP_ALLOWED_DOMAINS`.
    DomainNotAllowed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Features {
    pub signup: SignupMode,
    /// Email domains self-registration is limited to (`open` only); empty
    /// allows every domain.
    pub signup_allowed_domains: Vec<String>,
    pub passwordless_login: bool,
    pub console: bool,
    /// On when both the client id and secret are set.
    pub social_google: bool,
    pub social_github: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum FeaturesError {
    #[error(transparent)]
    Env(#[from] EnvError),
    #[error("`{set}` is set but `{missing}` is not: set both to enable the provider, or neither")]
    IncompleteProvider {
        set: &'static str,
        missing: &'static str,
    },
    #[error(
        "`SIGNUP_ALLOWED_DOMAINS` only applies with `SIGNUP_MODE=open` (it is `{mode}`): unset it or open signup"
    )]
    DomainsWithoutOpenSignup { mode: &'static str },
    #[error(
        "`PASSWORDLESS_LOGIN_ENABLED=true` has no effect with `MFA_MODE=required`: an emailed code alone never signs anyone in"
    )]
    PasswordlessWithRequiredMfa,
    #[error(
        "`PASSWORDLESS_LOGIN_ENABLED=true` needs `EMAIL_OTP_PEPPER`, which secures the emailed codes"
    )]
    PasswordlessWithoutOtpPepper,
}

impl Features {
    pub fn from_env() -> Result<Self, FeaturesError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, FeaturesError> {
        let var = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let flag = |name: &'static str, default: bool| match var(name) {
            None => Ok(default),
            Some(value) => parse_bool(&value).ok_or_else(|| EnvError::Parse {
                name: name.to_string(),
                value,
                message: "expected true or false".to_string(),
            }),
        };
        let provider = |id: &'static str, secret: &'static str| match (var(id), var(secret)) {
            (Some(_), Some(_)) => Ok(true),
            (None, None) => Ok(false),
            (Some(_), None) => Err(FeaturesError::IncompleteProvider {
                set: id,
                missing: secret,
            }),
            (None, Some(_)) => Err(FeaturesError::IncompleteProvider {
                set: secret,
                missing: id,
            }),
        };
        let signup = match var("SIGNUP_MODE") {
            None => SignupMode::default(),
            Some(value) => SignupMode::parse(&value).ok_or_else(|| EnvError::Parse {
                name: "SIGNUP_MODE".to_string(),
                value,
                message: "expected open, invite, or closed".to_string(),
            })?,
        };
        let signup_allowed_domains = match var("SIGNUP_ALLOWED_DOMAINS") {
            None => Vec::new(),
            Some(value) => parse_domains(&value).map_err(|message| EnvError::Parse {
                name: "SIGNUP_ALLOWED_DOMAINS".to_string(),
                value,
                message,
            })?,
        };
        if !signup_allowed_domains.is_empty() && signup != SignupMode::Open {
            return Err(FeaturesError::DomainsWithoutOpenSignup {
                mode: signup.as_str(),
            });
        }
        let passwordless_login = flag("PASSWORDLESS_LOGIN_ENABLED", false)?;
        // `iam::otp::policy` reads MFA_MODE; only `required` matters here.
        if passwordless_login
            && var("MFA_MODE").is_some_and(|mode| mode.eq_ignore_ascii_case("required"))
        {
            return Err(FeaturesError::PasswordlessWithRequiredMfa);
        }
        // Read by `iam::otp::util::email_otp_pepper` when a code is issued.
        if passwordless_login && var("EMAIL_OTP_PEPPER").is_none() {
            return Err(FeaturesError::PasswordlessWithoutOtpPepper);
        }
        Ok(Self {
            signup,
            signup_allowed_domains,
            passwordless_login,
            console: flag("CONSOLE_ENABLED", true)?,
            social_google: provider("GOOGLE_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_SECRET")?,
            social_github: provider("GITHUB_OAUTH_CLIENT_ID", "GITHUB_OAUTH_CLIENT_SECRET")?,
        })
    }

    pub fn enabled(&self, feature: Feature) -> bool {
        match feature {
            Feature::Signup => self.signup == SignupMode::Open,
            Feature::Invitations => self.signup != SignupMode::Closed,
            Feature::PasswordlessLogin => self.passwordless_login,
            Feature::Console => self.console,
            Feature::Social => self.social_google || self.social_github,
            Feature::SocialGoogle => self.social_google,
            Feature::SocialGithub => self.social_github,
        }
    }

    /// Whether someone may create their own account for `email` (canonical):
    /// `POST /signup` and the first social sign-in.
    pub fn self_service_signup(&self, email: &str) -> Result<(), SelfServiceRefusal> {
        if self.signup != SignupMode::Open {
            return Err(SelfServiceRefusal::Disabled);
        }
        if self.signup_allowed_domains.is_empty() {
            return Ok(());
        }
        let domain = email
            .rsplit_once('@')
            .map(|(_, domain)| domain.to_ascii_lowercase())
            .unwrap_or_default();
        if self.signup_allowed_domains.contains(&domain) {
            Ok(())
        } else {
            Err(SelfServiceRefusal::DomainNotAllowed(domain))
        }
    }

    /// The enabled social sign-in providers, as the hosted apps name them.
    pub fn social_providers(&self) -> Vec<&'static str> {
        [
            ("google", self.social_google),
            ("github", self.social_github),
        ]
        .into_iter()
        .filter_map(|(name, enabled)| enabled.then_some(name))
        .collect()
    }
}

impl fmt::Display for Features {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let on_off = |enabled: bool| if enabled { "on" } else { "off" };
        write!(f, "signup={}", self.signup.as_str())?;
        if !self.signup_allowed_domains.is_empty() {
            write!(f, " (domains: {})", self.signup_allowed_domains.join(", "))?;
        }
        write!(
            f,
            " passwordless={} console={} social=[{}]",
            on_off(self.passwordless_login),
            on_off(self.console),
            self.social_providers().join(", ")
        )
    }
}

/// `SIGNUP_ALLOWED_DOMAINS`: comma-separated domains, matched exactly (no
/// wildcards, no subdomains); a leading `@` is accepted.
fn parse_domains(value: &str) -> Result<Vec<String>, String> {
    let mut domains = Vec::new();
    for entry in value.split(',') {
        let domain = entry.trim().trim_start_matches('@').to_ascii_lowercase();
        if domain.is_empty() {
            continue;
        }
        if domain.contains(['@', '*']) || domain.chars().any(char::is_whitespace) {
            return Err(format!(
                "`{domain}` is not a domain (exact domains only, such as example.com)"
            ));
        }
        if !domains.contains(&domain) {
            domains.push(domain);
        }
    }
    if domains.is_empty() {
        return Err("no domain listed".to_string());
    }
    Ok(domains)
}

static FEATURES: OnceLock<Features> = OnceLock::new();

/// Reads and validates the features; the server calls it once at startup, so
/// an invalid setting stops it before it serves anything.
pub fn init() -> Result<&'static Features, FeaturesError> {
    let features = Features::from_env()?;
    let features = FEATURES.get_or_init(|| features);
    tracing::info!("Features: {features}");
    if features.signup != SignupMode::Closed && std::env::var_os("SMTP_HOST").is_none() {
        tracing::warn!(
            "SIGNUP_MODE={} sends signup emails, but SMTP_HOST is unset (it falls back to 0.0.0.0:1025)",
            features.signup.as_str()
        );
    }
    Ok(features)
}

/// The features in effect. Outside the server (tests, CLI commands) they are
/// read on first use.
pub fn features() -> &'static Features {
    FEATURES
        .get_or_init(|| Features::from_env().expect("feature settings are validated at startup"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn features_from(vars: &[(&str, &str)]) -> Result<Features, FeaturesError> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        Features::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn defaults_invite_serve_the_console_without_social_providers() {
        let features = features_from(&[]).unwrap();
        assert_eq!(
            features,
            Features {
                signup: SignupMode::Invite,
                signup_allowed_domains: Vec::new(),
                passwordless_login: false,
                console: true,
                social_google: false,
                social_github: false,
            }
        );
        assert!(!features.enabled(Feature::Signup));
        assert!(features.enabled(Feature::Invitations));
        assert!(!features.enabled(Feature::PasswordlessLogin));
        assert!(!features.enabled(Feature::Social));
        assert_eq!(
            features.to_string(),
            "signup=invite passwordless=off console=on social=[]"
        );
    }

    #[test]
    fn signup_modes_gate_self_service_and_invitations() {
        for (mode, signup, invitations) in [
            ("open", true, true),
            ("Invite", false, true),
            ("closed", false, false),
        ] {
            let features = features_from(&[("SIGNUP_MODE", mode)]).unwrap();
            assert_eq!(features.enabled(Feature::Signup), signup, "{mode}");
            assert_eq!(
                features.enabled(Feature::Invitations),
                invitations,
                "{mode}"
            );
            assert_eq!(
                features.self_service_signup("ada@example.com").is_ok(),
                signup,
                "{mode}"
            );
        }
        let error = features_from(&[("SIGNUP_MODE", "public")]).unwrap_err();
        assert!(error.to_string().contains("SIGNUP_MODE"), "{error}");
    }

    #[test]
    fn allowed_domains_limit_self_service_in_open_mode_only() {
        let features = features_from(&[
            ("SIGNUP_MODE", "open"),
            (
                "SIGNUP_ALLOWED_DOMAINS",
                " Example.com, @corp.example ,, example.com",
            ),
        ])
        .unwrap();
        assert_eq!(
            features.signup_allowed_domains,
            ["example.com", "corp.example"]
        );
        assert_eq!(features.self_service_signup("ada@example.com"), Ok(()));
        assert_eq!(features.self_service_signup("ada@corp.example"), Ok(()));
        assert_eq!(
            features.self_service_signup("ada@sub.example.com"),
            Err(SelfServiceRefusal::DomainNotAllowed(
                "sub.example.com".to_string()
            ))
        );
        assert_eq!(
            features.to_string(),
            "signup=open (domains: example.com, corp.example) passwordless=off console=on social=[]"
        );

        assert!(matches!(
            features_from(&[("SIGNUP_ALLOWED_DOMAINS", "example.com")]).unwrap_err(),
            FeaturesError::DomainsWithoutOpenSignup { mode: "invite" }
        ));
        for invalid in ["*.example.com", "ada@example.com", "exa mple.com", " , "] {
            let error =
                features_from(&[("SIGNUP_MODE", "open"), ("SIGNUP_ALLOWED_DOMAINS", invalid)])
                    .unwrap_err();
            assert!(
                error.to_string().contains("SIGNUP_ALLOWED_DOMAINS"),
                "{invalid}: {error}"
            );
        }
    }

    #[test]
    fn flags_are_parsed_strictly() {
        for (value, expected) in [
            ("false", false),
            ("OFF", false),
            ("0", false),
            ("yes", true),
        ] {
            let features = features_from(&[("CONSOLE_ENABLED", value)]).unwrap();
            assert_eq!(features.console, expected, "{value}");
        }
        // Blank is unset, so the default applies.
        assert!(features_from(&[("CONSOLE_ENABLED", "  ")]).unwrap().console);

        let error = features_from(&[("CONSOLE_ENABLED", "flase")]).unwrap_err();
        assert!(error.to_string().contains("CONSOLE_ENABLED"), "{error}");
    }

    #[test]
    fn passwordless_login_is_off_by_default_and_needs_a_pepper_and_mfa_not_required() {
        let pepper = ("EMAIL_OTP_PEPPER", "pepper");
        for mfa_mode in ["off", "optional", ""] {
            let features = features_from(&[
                ("PASSWORDLESS_LOGIN_ENABLED", "true"),
                ("MFA_MODE", mfa_mode),
                pepper,
            ])
            .unwrap();
            assert!(features.enabled(Feature::PasswordlessLogin), "{mfa_mode}");
        }
        assert!(matches!(
            features_from(&[
                ("PASSWORDLESS_LOGIN_ENABLED", "on"),
                ("MFA_MODE", " Required "),
                pepper,
            ])
            .unwrap_err(),
            FeaturesError::PasswordlessWithRequiredMfa
        ));
        assert!(matches!(
            features_from(&[
                ("PASSWORDLESS_LOGIN_ENABLED", "true"),
                ("EMAIL_OTP_PEPPER", " ")
            ])
            .unwrap_err(),
            FeaturesError::PasswordlessWithoutOtpPepper
        ));
        // Off is fine whatever the MFA mode.
        assert!(features_from(&[("MFA_MODE", "required")]).is_ok());
        assert!(features_from(&[("PASSWORDLESS_LOGIN_ENABLED", "maybe")]).is_err());
    }

    #[test]
    fn a_provider_needs_both_its_client_id_and_secret() {
        let features = features_from(&[
            ("GITHUB_OAUTH_CLIENT_ID", "id"),
            ("GITHUB_OAUTH_CLIENT_SECRET", "secret"),
        ])
        .unwrap();
        assert!(features.enabled(Feature::SocialGithub));
        assert!(!features.enabled(Feature::SocialGoogle));
        assert!(features.enabled(Feature::Social));
        assert_eq!(features.social_providers(), ["github"]);

        let error = features_from(&[("GOOGLE_OAUTH_CLIENT_ID", "id")]).unwrap_err();
        assert!(
            matches!(
                error,
                FeaturesError::IncompleteProvider {
                    set: "GOOGLE_OAUTH_CLIENT_ID",
                    missing: "GOOGLE_OAUTH_CLIENT_SECRET",
                }
            ),
            "{error}"
        );
        assert!(features_from(&[("GITHUB_OAUTH_CLIENT_SECRET", "secret")]).is_err());
    }
}
