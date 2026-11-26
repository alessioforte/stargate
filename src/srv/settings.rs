//! Listener settings, read and validated once at startup like `conf::features`:
//! a name that cannot work stops startup instead of failing at bind time.
//!
//! Without `LISTENERS` there is one implicit listener on `0.0.0.0:$PORT` that
//! serves every plane, which is the single-listener behavior. Declaring
//! listeners makes each one explicit: an address, the planes it serves, and
//! whether it terminates TLS.

use crate::{
    api::plane::{Plane, PlaneSet},
    conf::env::{EnvError, parse_bool},
    srv::proxy::ProxyTrust,
};
use std::{net::SocketAddr, str::FromStr};

/// Connections one listener accepts at once (`MAX_CONNECTIONS`,
/// `LISTENER_<NAME>_MAX_CONNECTIONS`). It bounds file descriptors and tasks;
/// beyond it the kernel backlog holds further connections.
pub const DEFAULT_MAX_CONNECTIONS: usize = 1024;

/// Name of the listener used when `LISTENERS` is absent.
const IMPLICIT_NAME: &str = "default";

/// Longest listener name, so `LISTENER_<NAME>_<FIELD>` stays readable.
const MAX_NAME_LENGTH: usize = 32;

/// Fields a declared listener accepts after its name.
const LISTENER_FIELDS: [&str; 5] = [
    "ADDRESS",
    "PLANES",
    "TLS",
    "MAX_CONNECTIONS",
    "TRUSTED_PROXIES",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listener {
    pub name: String,
    pub address: SocketAddr,
    pub planes: PlaneSet,
    /// Terminates TLS on this listener (`LISTENER_<NAME>_TLS`). Defaults to on,
    /// except for the management plane: probes carry no secret and stay in
    /// clear text, so a management listener cannot enable TLS.
    pub tls: bool,
    pub max_connections: usize,
    /// Forwarding-header sources this listener accepts; empty trusts nobody.
    pub trusted_proxies: ProxyTrust,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    Env(#[from] EnvError),
    #[error(
        "`{0}` configures the implicit listener and cannot be used with `LISTENERS`: set it per listener instead"
    )]
    SingleListenerSetting(&'static str),
    #[error(
        "`LISTENERS` needs `JWT_ISSUER`: the default issuer uses `PORT`, which a declared listener does not use, and `OAUTH_BASE_URL` does not set the issuer"
    )]
    MissingIssuer,
    #[error("`LISTENERS` uses the listener name `{0}` more than once")]
    DuplicateName(String),
    #[error(
        "invalid listener name `{name}`: use 1 to {MAX_NAME_LENGTH} ASCII letters or digits, starting with a letter"
    )]
    InvalidName { name: String },
    #[error("listener `{name}` needs `{var}`")]
    MissingSetting { name: String, var: String },
    #[error(
        "listener `{name}` serves only the management plane, which never terminates TLS: probes carry no secret"
    )]
    ManagementTls { name: String },
    #[error("listener `{name}` serves the management plane with other planes: it must be alone")]
    ManagementWithOthers { name: String },
    #[error("listeners `{first}` and `{second}` share the address {address}")]
    DuplicateAddress {
        first: String,
        second: String,
        address: SocketAddr,
    },
    #[error(
        "`{key}` is not a listener setting: declare the listener in `LISTENERS` and use ADDRESS, PLANES, TLS, MAX_CONNECTIONS, or TRUSTED_PROXIES"
    )]
    UnknownSetting { key: String },
}

/// Reads and validates the listener settings. Call before opening any socket.
pub fn from_env() -> Result<Vec<Listener>, SettingsError> {
    let vars = std::env::vars_os()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect::<Vec<_>>();
    from_vars(&vars)
}

fn from_vars(vars: &[(String, String)]) -> Result<Vec<Listener>, SettingsError> {
    let var = |name: &str| {
        vars.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };

    match var("LISTENERS") {
        None => {
            // A per-listener variable without `LISTENERS` would be ignored
            // silently, including every listener the operator meant to declare.
            if let Some((key, _)) = vars.iter().find(|(key, _)| key.starts_with("LISTENER_")) {
                return Err(SettingsError::UnknownSetting { key: key.clone() });
            }
            implicit(&var).map(|listener| vec![listener])
        }
        Some(value) => declared(&value, vars, &var),
    }
}

/// Probes need a listener that declares the management plane. A declared
/// configuration without one answers `/livez` and `/readyz` nowhere, which
/// startup reports as a warning.
pub fn serves_probes(listeners: &[Listener]) -> bool {
    listeners
        .iter()
        .any(|listener| listener.planes.contains(Plane::Management))
}

/// The single listener of a process without `LISTENERS`: every plane on
/// `0.0.0.0:$PORT`, with the process-wide TLS and connection settings.
fn implicit(var: &impl Fn(&str) -> Option<String>) -> Result<Listener, SettingsError> {
    let port = match var("PORT") {
        None => 5050,
        Some(value) => parse_port("PORT", &value)?,
    };
    let address = SocketAddr::from(([0, 0, 0, 0], port));
    let tls = match var("TLS_ENABLED") {
        None => false,
        Some(value) => parse_flag("TLS_ENABLED", &value)?,
    };
    let max_connections = match var("MAX_CONNECTIONS") {
        None => DEFAULT_MAX_CONNECTIONS,
        Some(value) => parse_connections("MAX_CONNECTIONS", &value)?,
    };
    let trusted_proxies = parse_trust("TRUSTED_PROXIES", var("TRUSTED_PROXIES"))?;
    Ok(Listener {
        name: IMPLICIT_NAME.to_string(),
        address,
        planes: PlaneSet::ALL,
        tls,
        max_connections,
        trusted_proxies,
    })
}

fn declared(
    value: &str,
    vars: &[(String, String)],
    var: &impl Fn(&str) -> Option<String>,
) -> Result<Vec<Listener>, SettingsError> {
    for setting in ["PORT", "TLS_ENABLED", "MAX_CONNECTIONS", "TRUSTED_PROXIES"] {
        if var(setting).is_some() {
            return Err(SettingsError::SingleListenerSetting(setting));
        }
    }
    if var("JWT_ISSUER").is_none() {
        return Err(SettingsError::MissingIssuer);
    }

    let mut names = Vec::new();
    for name in value.split(',') {
        let name = name.trim().to_ascii_lowercase();
        validate_name(&name)?;
        if names.contains(&name) {
            return Err(SettingsError::DuplicateName(name));
        }
        names.push(name);
    }
    ensure_known_variables(vars, &names)?;

    let mut listeners: Vec<Listener> = Vec::with_capacity(names.len());
    for name in &names {
        let prefix = format!("LISTENER_{}_", name.to_ascii_uppercase());
        let required = |field: &str| {
            var(&format!("{prefix}{field}")).ok_or_else(|| SettingsError::MissingSetting {
                name: name.clone(),
                var: format!("{prefix}{field}"),
            })
        };

        let address = required("ADDRESS")?;
        let address = SocketAddr::from_str(&address).map_err(|_| {
            invalid(
                &format!("{prefix}ADDRESS"),
                &address,
                "expected a complete address with a port, e.g. 0.0.0.0:5050 or [::1]:5050",
            )
        })?;
        if address.port() == 0 {
            return Err(invalid(
                &format!("{prefix}ADDRESS"),
                &address.to_string(),
                "expected a port from 1 to 65535",
            )
            .into());
        }
        if let Some(other) = listeners
            .iter()
            .find(|listener: &&Listener| listener.address == address)
        {
            return Err(SettingsError::DuplicateAddress {
                first: other.name.clone(),
                second: name.clone(),
                address,
            });
        }

        let planes = required("PLANES")?;
        let planes = PlaneSet::parse(&planes)
            .map_err(|message| invalid(&format!("{prefix}PLANES"), &planes, &message))?;
        if planes.contains(Plane::Management) && !planes.only(Plane::Management) {
            return Err(SettingsError::ManagementWithOthers { name: name.clone() });
        }
        let tls = match var(&format!("{prefix}TLS")) {
            None => !planes.only(Plane::Management),
            Some(value) => parse_flag(&format!("{prefix}TLS"), &value)?,
        };
        if tls && planes.only(Plane::Management) {
            return Err(SettingsError::ManagementTls { name: name.clone() });
        }
        let max_connections = match var(&format!("{prefix}MAX_CONNECTIONS")) {
            None => DEFAULT_MAX_CONNECTIONS,
            Some(value) => parse_connections(&format!("{prefix}MAX_CONNECTIONS"), &value)?,
        };
        let trusted_proxies = parse_trust(
            &format!("{prefix}TRUSTED_PROXIES"),
            var(&format!("{prefix}TRUSTED_PROXIES")),
        )?;

        listeners.push(Listener {
            name: name.clone(),
            address,
            planes,
            tls,
            max_connections,
            trusted_proxies,
        });
    }
    Ok(listeners)
}

pub(crate) fn validate_name(name: &str) -> Result<(), SettingsError> {
    let mut chars = name.chars();
    let valid = name.len() <= MAX_NAME_LENGTH
        && chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|rest| rest.is_ascii_alphanumeric());
    if valid {
        Ok(())
    } else {
        Err(SettingsError::InvalidName {
            name: name.to_string(),
        })
    }
}

/// Rejects `LISTENER_*` variables that no declared listener reads, so a typo
/// (a misspelled name or field) stops startup instead of silently using a
/// default.
fn ensure_known_variables(
    vars: &[(String, String)],
    names: &[String],
) -> Result<(), SettingsError> {
    for (key, _) in vars {
        if !key.starts_with("LISTENER_") {
            continue;
        }
        let known = names.iter().any(|declared| {
            let prefix = format!("LISTENER_{}_", declared.to_ascii_uppercase());
            key.strip_prefix(&prefix)
                .is_some_and(|field| LISTENER_FIELDS.contains(&field))
        });
        if !known {
            return Err(SettingsError::UnknownSetting { key: key.clone() });
        }
    }
    Ok(())
}

fn parse_port(name: &str, value: &str) -> Result<u16, SettingsError> {
    match value.parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err(invalid(name, value, "expected a port from 1 to 65535").into()),
    }
}

fn parse_flag(name: &str, value: &str) -> Result<bool, SettingsError> {
    parse_bool(value).ok_or_else(|| invalid(name, value, "expected true or false").into())
}

fn parse_connections(name: &str, value: &str) -> Result<usize, SettingsError> {
    match value.parse::<usize>() {
        Ok(0) | Err(_) => Err(invalid(
            name,
            value,
            "expected a positive number of concurrent connections",
        )
        .into()),
        Ok(connections) => Ok(connections),
    }
}

fn parse_trust(name: &str, value: Option<String>) -> Result<ProxyTrust, SettingsError> {
    let Some(value) = value else {
        return Ok(ProxyTrust::default());
    };
    ProxyTrust::parse(&value).map_err(|message| invalid(name, &value, &message).into())
}

fn invalid(name: &str, value: &str, message: &str) -> EnvError {
    EnvError::Parse {
        name: name.to_string(),
        value: value.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MAX_CONNECTIONS, Listener, SettingsError, from_vars, serves_probes};
    use crate::api::plane::{Plane, PlaneSet};
    use std::{net::SocketAddr, str::FromStr};

    fn listeners(vars: &[(&str, &str)]) -> Result<Vec<Listener>, SettingsError> {
        let mut vars = vars
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect::<Vec<_>>();
        // A declared listener needs `JWT_ISSUER` (see
        // `declared_listeners_need_an_explicit_issuer`); tests that do not
        // exercise that rule get one here.
        if vars.iter().any(|(key, _)| key == "LISTENERS")
            && !vars.iter().any(|(key, _)| key == "JWT_ISSUER")
        {
            vars.push((
                "JWT_ISSUER".to_string(),
                "https://identity.test".to_string(),
            ));
        }
        from_vars(&vars)
    }

    fn address(value: &str) -> SocketAddr {
        SocketAddr::from_str(value).unwrap()
    }

    #[test]
    fn without_listeners_one_listener_serves_every_plane_on_the_port() {
        let parsed = listeners(&[]).unwrap();
        assert_eq!(parsed.len(), 1);
        let listener = &parsed[0];
        assert_eq!(listener.name, "default");
        assert_eq!(listener.address, address("0.0.0.0:5050"));
        assert_eq!(listener.planes, PlaneSet::ALL);
        assert!(!listener.tls);
        assert_eq!(listener.max_connections, DEFAULT_MAX_CONNECTIONS);

        let listener = &listeners(&[("PORT", "8080")]).unwrap()[0];
        assert_eq!(listener.address, address("0.0.0.0:8080"));

        let listener = &listeners(&[("TLS_ENABLED", "on"), ("MAX_CONNECTIONS", "50")]).unwrap()[0];
        assert!(listener.tls);
        assert_eq!(listener.max_connections, 50);

        // An empty value is an absent setting, as everywhere else.
        let listener = &listeners(&[("LISTENERS", "  ")]).unwrap()[0];
        assert_eq!(listener.planes, PlaneSet::ALL);

        // A per-listener setting without LISTENERS must not be ignored.
        let error = listeners(&[("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050")]).unwrap_err();
        assert!(matches!(error, SettingsError::UnknownSetting { .. }));
    }

    #[test]
    fn declared_listeners_carry_their_own_address_planes_and_tls() {
        let parsed = listeners(&[
            ("LISTENERS", "public,admin,internal,management"),
            ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
            ("LISTENER_PUBLIC_PLANES", "identity,gateway"),
            ("LISTENER_ADMIN_ADDRESS", "127.0.0.1:5051"),
            ("LISTENER_ADMIN_PLANES", "admin"),
            ("LISTENER_INTERNAL_ADDRESS", "[::1]:5052"),
            ("LISTENER_INTERNAL_PLANES", "admin"),
            ("LISTENER_INTERNAL_TLS", "off"),
            ("LISTENER_MANAGEMENT_ADDRESS", "0.0.0.0:9000"),
            ("LISTENER_MANAGEMENT_PLANES", "management"),
            ("LISTENER_MANAGEMENT_MAX_CONNECTIONS", "64"),
        ])
        .unwrap();

        let names = parsed
            .iter()
            .map(|listener| listener.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["public", "admin", "internal", "management"]);
        assert_eq!(
            parsed[0].planes,
            PlaneSet::parse("identity,gateway").unwrap()
        );
        assert_eq!(parsed[0].address, address("0.0.0.0:5050"));
        // TLS is on by default for everything but the management plane.
        assert!(parsed[0].tls);
        assert!(parsed[1].tls);
        assert!(!parsed[2].tls);
        assert_eq!(parsed[2].address, address("[::1]:5052"));
        assert!(!parsed[3].tls);
        assert_eq!(parsed[3].max_connections, 64);
        assert_eq!(parsed[3].planes, PlaneSet::of(Plane::Management));
    }

    #[test]
    fn declared_listeners_without_a_management_one_serve_no_probes() {
        // Probes answer only on a listener that declares the management plane:
        // splitting the planes without one leaves `/livez` and `/readyz` dark.
        let parsed = listeners(&[
            ("LISTENERS", "public,admin"),
            ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
            ("LISTENER_PUBLIC_PLANES", "identity,gateway"),
            ("LISTENER_ADMIN_ADDRESS", "127.0.0.1:5051"),
            ("LISTENER_ADMIN_PLANES", "admin"),
        ])
        .unwrap();
        assert!(!serves_probes(&parsed));

        let parsed = listeners(&[
            ("LISTENERS", "public,management"),
            ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
            ("LISTENER_PUBLIC_PLANES", "identity,gateway"),
            ("LISTENER_MANAGEMENT_ADDRESS", "0.0.0.0:9000"),
            ("LISTENER_MANAGEMENT_PLANES", "management"),
        ])
        .unwrap();
        assert!(serves_probes(&parsed));
        assert!(!parsed[0].planes.contains(Plane::Management));
        assert!(parsed[1].planes.only(Plane::Management));
        assert!(!parsed[1].tls);

        // The implicit listener serves every plane, probes included.
        assert!(serves_probes(&listeners(&[]).unwrap()));
    }

    #[test]
    fn single_listener_settings_stop_a_declared_configuration() {
        for (setting, value) in [
            ("PORT", "5050"),
            ("TLS_ENABLED", "true"),
            ("MAX_CONNECTIONS", "10"),
            ("TRUSTED_PROXIES", "10.0.0.0/8"),
        ] {
            let error = listeners(&[
                ("LISTENERS", "public"),
                ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
                ("LISTENER_PUBLIC_PLANES", "identity"),
                (setting, value),
            ])
            .unwrap_err();
            assert!(
                matches!(error, SettingsError::SingleListenerSetting(name) if name == setting),
                "{setting}: {error}"
            );
        }
    }

    #[test]
    fn listener_names_are_validated_and_unique() {
        for value in [
            ",",
            "public,",
            "Public Listener",
            "1public",
            "pub-lic",
            "public,,admin",
        ] {
            assert!(listeners(&[("LISTENERS", value)]).is_err(), "{value}");
        }
        let long = "a".repeat(33);
        assert!(listeners(&[("LISTENERS", &long)]).is_err());
        let error = listeners(&[("LISTENERS", "public,public")]).unwrap_err();
        assert!(matches!(error, SettingsError::DuplicateName(name) if name == "public"));
        let error = listeners(&[("LISTENERS", "public,Public")]).unwrap_err();
        assert!(matches!(error, SettingsError::DuplicateName(name) if name == "public"));
    }

    #[test]
    fn every_listener_needs_a_complete_address_and_planes() {
        for (field, value) in [
            ("ADDRESS", "localhost:5050"),
            ("ADDRESS", "0.0.0.0"),
            ("ADDRESS", "5050"),
            ("ADDRESS", "::1:5050"),
            ("ADDRESS", "0.0.0.0:0"),
            ("ADDRESS", "0.0.0.0:70000"),
        ] {
            assert!(
                listeners(&[
                    ("LISTENERS", "public"),
                    ("LISTENER_PUBLIC_ADDRESS", value),
                    ("LISTENER_PUBLIC_PLANES", "identity"),
                ])
                .is_err(),
                "{field}={value}"
            );
        }
        for field in ["ADDRESS", "PLANES"] {
            let vars = [
                ("LISTENERS", "public"),
                ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
                ("LISTENER_PUBLIC_PLANES", "identity"),
            ];
            let vars = vars
                .into_iter()
                .filter(|(key, _)| *key != format!("LISTENER_PUBLIC_{field}"))
                .collect::<Vec<_>>();
            let error = listeners(&vars).unwrap_err();
            assert!(
                matches!(error, SettingsError::MissingSetting { ref var, .. } if var == &format!("LISTENER_PUBLIC_{field}")),
                "{field}: {error}"
            );
        }
    }

    #[test]
    fn planes_are_known_distinct_and_management_stands_alone() {
        for value in ["", ",", "identity,", "billing", "identity,identity"] {
            assert!(
                listeners(&[
                    ("LISTENERS", "public"),
                    ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
                    ("LISTENER_PUBLIC_PLANES", value),
                ])
                .is_err(),
                "{value}"
            );
        }
        let error = listeners(&[
            ("LISTENERS", "probes"),
            ("LISTENER_PROBES_ADDRESS", "0.0.0.0:9000"),
            ("LISTENER_PROBES_PLANES", "management,identity"),
        ])
        .unwrap_err();
        assert!(matches!(error, SettingsError::ManagementWithOthers { .. }));

        let error = listeners(&[
            ("LISTENERS", "probes"),
            ("LISTENER_PROBES_ADDRESS", "0.0.0.0:9000"),
            ("LISTENER_PROBES_PLANES", "management"),
            ("LISTENER_PROBES_TLS", "on"),
        ])
        .unwrap_err();
        assert!(matches!(error, SettingsError::ManagementTls { .. }));
    }

    #[test]
    fn per_listener_flags_and_limits_are_parsed_strictly() {
        for (field, value) in [
            ("TLS", "sometimes"),
            ("TLS", "1.0"),
            ("MAX_CONNECTIONS", "0"),
            ("MAX_CONNECTIONS", "-1"),
            ("MAX_CONNECTIONS", "many"),
        ] {
            let key = format!("LISTENER_PUBLIC_{field}");
            let error = listeners(&[
                ("LISTENERS", "public"),
                ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
                ("LISTENER_PUBLIC_PLANES", "identity"),
                (key.as_str(), value),
            ])
            .unwrap_err();
            assert!(
                matches!(error, SettingsError::Env(_)),
                "{field}={value}: {error}"
            );
        }
        let parsed = listeners(&[
            ("LISTENERS", "public"),
            ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
            ("LISTENER_PUBLIC_PLANES", "identity"),
            ("LISTENER_PUBLIC_TLS", "false"),
            ("LISTENER_PUBLIC_MAX_CONNECTIONS", "32"),
        ])
        .unwrap();
        assert!(!parsed[0].tls);
        assert_eq!(parsed[0].max_connections, 32);
    }

    #[test]
    fn listeners_cannot_share_an_address() {
        let error = listeners(&[
            ("LISTENERS", "public,admin"),
            ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
            ("LISTENER_PUBLIC_PLANES", "identity"),
            ("LISTENER_ADMIN_ADDRESS", "0.0.0.0:5050"),
            ("LISTENER_ADMIN_PLANES", "admin"),
        ])
        .unwrap_err();
        assert!(matches!(error, SettingsError::DuplicateAddress { .. }));
    }

    #[test]
    fn unknown_listener_variables_stop_startup() {
        for (key, value) in [
            ("LISTENER_PUBLIC_PROXIES", "10.0.0.0/8"),
            ("LISTENER_PUBLIC_TRUSTED_PROXY", "10.0.0.0/8"),
            ("LISTENER_PRIVATE_ADDRESS", "0.0.0.0:5051"),
            ("LISTENER_PUBLICADRESS", "0.0.0.0:5050"),
            ("LISTENER_PUBLIC_ADDRESS_TLS", "on"),
        ] {
            let error = listeners(&[
                ("LISTENERS", "public"),
                ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
                ("LISTENER_PUBLIC_PLANES", "identity"),
                (key, value),
            ])
            .unwrap_err();
            assert!(
                matches!(error, SettingsError::UnknownSetting { .. }),
                "{key}: {error}"
            );
        }
    }

    #[test]
    fn the_implicit_listener_reads_its_proxy_trust_once() {
        let parsed = listeners(&[("TRUSTED_PROXIES", "127.0.0.1, 10.42.0.0/16")]).unwrap();
        assert_eq!(parsed[0].trusted_proxies.source_count(), 2);
        assert!(
            parsed[0]
                .trusted_proxies
                .trusts("10.42.9.8".parse().unwrap())
        );
        assert!(
            !parsed[0]
                .trusted_proxies
                .trusts("10.43.0.1".parse().unwrap())
        );

        assert_eq!(listeners(&[]).unwrap()[0].trusted_proxies.source_count(), 0);

        // Unlike the old process-wide list, an invalid entry stops startup.
        let error = listeners(&[("TRUSTED_PROXIES", "10.0.0.0/8, not-an-ip")]).unwrap_err();
        assert!(matches!(error, SettingsError::Env(_)), "{error}");
    }

    #[test]
    fn declared_listeners_carry_their_own_proxy_trust() {
        let parsed = listeners(&[
            ("LISTENERS", "public,internal"),
            ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
            ("LISTENER_PUBLIC_PLANES", "identity,gateway"),
            ("LISTENER_PUBLIC_TRUSTED_PROXIES", "10.42.0.0/16"),
            ("LISTENER_INTERNAL_ADDRESS", "0.0.0.0:5052"),
            ("LISTENER_INTERNAL_PLANES", "admin"),
        ])
        .unwrap();
        assert_eq!(parsed[0].trusted_proxies.source_count(), 1);
        assert!(
            parsed[0]
                .trusted_proxies
                .trusts("10.42.1.2".parse().unwrap())
        );
        // A listener without the setting trusts nobody; there is no default.
        assert_eq!(parsed[1].trusted_proxies.source_count(), 0);

        let error = listeners(&[
            ("LISTENERS", "public"),
            ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
            ("LISTENER_PUBLIC_PLANES", "identity"),
            ("LISTENER_PUBLIC_TRUSTED_PROXIES", "10.42.0.0/16, nonsense"),
        ])
        .unwrap_err();
        assert!(matches!(error, SettingsError::Env(_)), "{error}");

        // With LISTENERS the trust has one source: the per-listener setting.
        let error = listeners(&[
            ("LISTENERS", "public"),
            ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
            ("LISTENER_PUBLIC_PLANES", "identity"),
            ("TRUSTED_PROXIES", "10.42.0.0/16"),
        ])
        .unwrap_err();
        assert!(
            matches!(
                error,
                SettingsError::SingleListenerSetting("TRUSTED_PROXIES")
            ),
            "{error}"
        );
    }

    #[test]
    fn declared_listeners_need_an_explicit_issuer() {
        let base = [
            ("LISTENERS", "public"),
            ("LISTENER_PUBLIC_ADDRESS", "0.0.0.0:5050"),
            ("LISTENER_PUBLIC_PLANES", "identity"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect::<Vec<_>>();
        let error = from_vars(&base).unwrap_err();
        assert!(matches!(error, SettingsError::MissingIssuer), "{error}");

        // `OAUTH_BASE_URL` does not set the issuer: `issuer_from_env` reads
        // `JWT_ISSUER` only, so the localhost default would remain.
        let mut vars = base.clone();
        vars.push((
            "OAUTH_BASE_URL".to_string(),
            "https://identity.example.com".to_string(),
        ));
        let error = from_vars(&vars).unwrap_err();
        assert!(matches!(error, SettingsError::MissingIssuer), "{error}");

        vars.push((
            "JWT_ISSUER".to_string(),
            "https://identity.example.com".to_string(),
        ));
        assert!(from_vars(&vars).is_ok());
    }
}
