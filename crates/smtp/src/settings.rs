//! SMTP settings, read and validated once at startup like the other
//! process-wide settings: an invalid host, port or sender, or `SMTP_TLS=none`
//! with credentials, stops startup instead of failing or leaking credentials
//! on the first email.

use lettre::address::AddressError;
use lettre::message::Mailbox;
use std::fmt;
use std::sync::OnceLock;
use tools::Locale;

const DEFAULT_HOST: &str = "0.0.0.0";
const DEFAULT_PORT: &str = "1025";
const DEFAULT_FROM: &str = "NoBody <nobody@domain.tld>";

/// How the SMTP transport protects the connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SmtpTls {
    /// Connect in plaintext and require `STARTTLS` before sending anything.
    /// This is mandatory, not opportunistic: a server without `STARTTLS`
    /// fails the send.
    #[default]
    Starttls,
    /// Connect directly to a TLS-wrapped socket (SMTPS).
    Implicit,
    /// Plaintext only, for a trusted local relay such as Mailpit.
    None,
}

impl SmtpTls {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starttls => "starttls",
            Self::Implicit => "implicit",
            Self::None => "none",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "starttls" => Some(Self::Starttls),
            "implicit" => Some(Self::Implicit),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    /// Both TLS modes use the host as the TLS server name, for the SNI and for
    /// certificate verification.
    const fn uses_tls(self) -> bool {
        matches!(self, Self::Starttls | Self::Implicit)
    }
}

impl fmt::Display for SmtpTls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SmtpSettings {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) from: Mailbox,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) tls: SmtpTls,
    pub(crate) default_locale: Locale,
}

#[derive(Debug, thiserror::Error)]
pub enum SmtpSettingsError {
    #[error("environment variable `{name}` contains invalid value `{value}`: {message}")]
    Parse {
        name: String,
        value: String,
        message: String,
    },
    #[error(
        "`SMTP_TLS=none` sends SMTP credentials in plaintext: set SMTP_TLS=starttls or implicit, or unset SMTP_USERNAME and SMTP_PASSWORD"
    )]
    CredentialsWithoutTls,
}

impl SmtpSettings {
    pub fn default_locale(&self) -> Locale {
        self.default_locale
    }

    pub fn from_env() -> Result<Self, SmtpSettingsError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, SmtpSettingsError> {
        let var = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let invalid = |name: &str, value: &str, message: String| SmtpSettingsError::Parse {
            name: name.to_string(),
            value: value.to_string(),
            message,
        };

        let tls = match var("SMTP_TLS") {
            None => SmtpTls::Starttls,
            Some(value) => SmtpTls::parse(&value).ok_or_else(|| {
                invalid(
                    "SMTP_TLS",
                    &value,
                    "expected starttls, implicit, or none".to_string(),
                )
            })?,
        };
        let host = var("SMTP_HOST").unwrap_or_else(|| DEFAULT_HOST.to_string());
        if tls.uses_tls() {
            // The same check rustls repeats when connecting: without a usable
            // server name there is no SNI and no certificate verification.
            rustls::pki_types::ServerName::try_from(host.clone()).map_err(|_| {
                invalid(
                    "SMTP_HOST",
                    &host,
                    format!("not a hostname or IP address TLS can verify (SMTP_TLS={tls})"),
                )
            })?;
        }
        let port_value = var("SMTP_PORT").unwrap_or_else(|| DEFAULT_PORT.to_string());
        let port = match port_value.parse::<u16>() {
            Ok(port) if port != 0 => port,
            _ => {
                return Err(invalid(
                    "SMTP_PORT",
                    &port_value,
                    "expected a port number from 1 to 65535".to_string(),
                ));
            }
        };
        let from_value = var("SMTP_FROM").unwrap_or_else(|| DEFAULT_FROM.to_string());
        let from: Mailbox = from_value
            .parse()
            .map_err(|error: AddressError| invalid("SMTP_FROM", &from_value, error.to_string()))?;
        let username = var("SMTP_USERNAME").unwrap_or_default();
        // Taken verbatim: trimming would change the credential.
        let password = lookup("SMTP_PASSWORD").unwrap_or_default();
        if tls == SmtpTls::None && (!username.is_empty() || !password.is_empty()) {
            return Err(SmtpSettingsError::CredentialsWithoutTls);
        }
        let default_locale_value =
            var("DEFAULT_LOCALE").unwrap_or_else(|| Locale::FALLBACK.to_string());
        let default_locale = Locale::parse(&default_locale_value).ok_or_else(|| {
            invalid(
                "DEFAULT_LOCALE",
                &default_locale_value,
                format!("expected one of: {}", locale_values()),
            )
        })?;
        Ok(Self {
            host,
            port,
            from,
            username,
            password,
            tls,
            default_locale,
        })
    }
}

fn locale_values() -> String {
    Locale::ALL
        .iter()
        .map(|locale| locale.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for SmtpSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let auth = if self.username.is_empty() && self.password.is_empty() {
            "off"
        } else {
            "on"
        };
        write!(
            f,
            "host={} port={} tls={} from={} auth={auth} default_locale={}",
            self.host, self.port, self.tls, self.from, self.default_locale
        )
    }
}

impl fmt::Debug for SmtpSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpSettings")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("from", &self.from)
            .field("username", &"<redacted>")
            .field("password", &"<redacted>")
            .field("tls", &self.tls)
            .field("default_locale", &self.default_locale)
            .finish()
    }
}

/// The installation's configured default language, then English.
pub fn default_locale() -> Locale {
    smtp_settings().default_locale
}

static SETTINGS: OnceLock<SmtpSettings> = OnceLock::new();

/// Reads and validates the settings; the server calls it once at startup, so
/// an invalid value stops it before it serves anything.
pub fn init() -> Result<&'static SmtpSettings, SmtpSettingsError> {
    let settings = SmtpSettings::from_env()?;
    let settings = SETTINGS.get_or_init(|| settings);
    tracing::info!("SMTP: {settings}");
    Ok(settings)
}

/// The settings in effect. Outside the server (tests) they are read on first
/// use.
pub fn smtp_settings() -> &'static SmtpSettings {
    SETTINGS
        .get_or_init(|| SmtpSettings::from_env().expect("SMTP settings are validated at startup"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn settings_from(vars: &[(&str, &str)]) -> Result<SmtpSettings, SmtpSettingsError> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        SmtpSettings::from_lookup(|name| vars.get(name).cloned())
    }

    fn message(vars: &[(&str, &str)]) -> String {
        settings_from(vars).unwrap_err().to_string()
    }

    fn non_tls_settings() -> SmtpSettings {
        settings_from(&[("SMTP_TLS", "none")]).unwrap()
    }

    #[test]
    fn defaults_to_starttls_without_credentials() {
        let settings = non_tls_settings();
        assert_eq!(settings.host, DEFAULT_HOST);
        assert_eq!(settings.port, 1025);
        assert_eq!(settings.from, DEFAULT_FROM.parse::<Mailbox>().unwrap());
        assert_eq!(settings.username, "");
        assert_eq!(settings.password, "");
        assert_eq!(settings.default_locale(), Locale::En);
        assert_eq!(
            settings.to_string(),
            "host=0.0.0.0 port=1025 tls=none from=NoBody <nobody@domain.tld> auth=off default_locale=en"
        );

        let defaults = settings_from(&[]).unwrap();
        assert_eq!(defaults.tls, SmtpTls::Starttls);
        assert_eq!(defaults.port, 1025);
    }

    #[test]
    fn parses_the_default_locale() {
        for (value, expected) in [
            ("en", Locale::En),
            ("IT", Locale::It),
            ("it-IT", Locale::It),
            ("en-US", Locale::En),
        ] {
            let settings = settings_from(&[("SMTP_TLS", "none"), ("DEFAULT_LOCALE", value)])
                .unwrap_or_else(|error| panic!("{value}: {error}"));
            assert_eq!(settings.default_locale(), expected, "{value}");
        }
    }

    #[test]
    fn rejects_an_unsupported_default_locale() {
        let error = message(&[("SMTP_TLS", "none"), ("DEFAULT_LOCALE", "fr")]);
        assert!(error.contains("DEFAULT_LOCALE"), "{error}");
    }

    #[test]
    fn parses_tls_modes_case_insensitively() {
        for (value, expected) in [
            ("starttls", SmtpTls::Starttls),
            ("STARTTLS", SmtpTls::Starttls),
            ("implicit", SmtpTls::Implicit),
            ("Implicit", SmtpTls::Implicit),
            ("none", SmtpTls::None),
            ("NONE", SmtpTls::None),
        ] {
            let settings = settings_from(&[("SMTP_TLS", value)]).unwrap();
            assert_eq!(settings.tls, expected, "{value}");
        }
    }

    #[test]
    fn rejects_invalid_tls_values() {
        for value in ["tls", "opportunistic", "start_tls", "smtps"].iter() {
            let error = message(&[("SMTP_TLS", value)]);
            assert!(error.contains("SMTP_TLS"), "{value}: {error}");
        }
    }

    #[test]
    fn rejects_invalid_ports() {
        for value in ["not-a-port", "0", "65536", "587.5", "-1"].iter() {
            let error = message(&[("SMTP_TLS", "none"), ("SMTP_PORT", value)]);
            assert!(error.contains("SMTP_PORT"), "{value}: {error}");
        }
        assert_eq!(
            settings_from(&[("SMTP_TLS", "none"), ("SMTP_PORT", " 587 ")])
                .unwrap()
                .port,
            587
        );
    }

    #[test]
    fn rejects_invalid_senders() {
        for value in ["not-an-address", "Name <>", "a@b@c"].iter() {
            let error = message(&[("SMTP_TLS", "none"), ("SMTP_FROM", value)]);
            assert!(error.contains("SMTP_FROM"), "{value}: {error}");
        }
        assert_eq!(
            settings_from(&[
                ("SMTP_TLS", "none"),
                ("SMTP_FROM", "Stargate <no-reply@example.com>")
            ])
            .unwrap()
            .from
            .to_string(),
            "Stargate <no-reply@example.com>"
        );
    }

    #[test]
    fn rejects_hosts_tls_cannot_verify() {
        for tls in ["starttls", "implicit"].iter() {
            for value in [
                "not a host",
                "smtp.example.com:587",
                "https://smtp.example.com",
            ]
            .iter()
            {
                let error = message(&[("SMTP_TLS", tls), ("SMTP_HOST", value)]);
                assert!(error.contains("SMTP_HOST"), "{tls} {value}: {error}");
            }
            // A hostname and an IP address are both valid TLS server names.
            for value in ["smtp.example.com", "192.0.2.10"].iter() {
                assert!(
                    settings_from(&[("SMTP_TLS", tls), ("SMTP_HOST", value)]).is_ok(),
                    "{tls} {value}"
                );
            }
        }
        // Without TLS the host is not a server name.
        assert!(settings_from(&[("SMTP_TLS", "none"), ("SMTP_HOST", "not a host")]).is_ok());
    }

    #[test]
    fn rejects_none_with_credentials() {
        for vars in [
            &[
                ("SMTP_TLS", "none"),
                ("SMTP_USERNAME", "project-id"),
                ("SMTP_PASSWORD", "secret"),
            ][..],
            &[("SMTP_TLS", "none"), ("SMTP_USERNAME", "project-id")][..],
            &[("SMTP_TLS", "none"), ("SMTP_PASSWORD", "secret")][..],
        ] {
            let error = settings_from(vars).unwrap_err();
            assert!(
                matches!(error, SmtpSettingsError::CredentialsWithoutTls),
                "{vars:?}: {error}"
            );
        }
    }

    #[test]
    fn allows_none_without_credentials_and_tls_with_credentials() {
        assert_eq!(non_tls_settings().tls, SmtpTls::None);
        let settings =
            settings_from(&[("SMTP_USERNAME", "project-id"), ("SMTP_PASSWORD", "secret")]).unwrap();
        assert_eq!(settings.tls, SmtpTls::Starttls);
        assert_eq!(settings.username, "project-id");
        assert_eq!(settings.password, "secret");
        assert!(settings.to_string().contains("auth=on"));
        assert!(!settings.to_string().contains("secret"));
        assert!(!format!("{settings:?}").contains("secret"));
        assert!(!format!("{settings:?}").contains("project-id"));
    }

    #[test]
    fn keeps_the_password_verbatim() {
        let settings = settings_from(&[
            ("SMTP_USERNAME", "project-id"),
            ("SMTP_PASSWORD", " secret with spaces "),
        ])
        .unwrap();
        assert_eq!(settings.password, " secret with spaces ");
        assert_eq!(settings.username, "project-id");
    }
}
