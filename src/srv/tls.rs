use std::{env, io, path::Path, sync::OnceLock};

pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn parse_bool_env(env_key: &str, value: &str) -> io::Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{env_key} must be a boolean value, got: {value}"),
        )),
    }
}

static LISTENER_TLS: OnceLock<io::Result<bool>> = OnceLock::new();

fn cached_enabled(
    cache: &OnceLock<io::Result<bool>>,
    read: impl FnOnce() -> io::Result<bool>,
) -> io::Result<bool> {
    match cache.get_or_init(read) {
        Ok(enabled) => Ok(*enabled),
        Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
    }
}

/// Validate and cache the listener setting before serving requests.
pub fn enabled() -> io::Result<bool> {
    cached_enabled(&LISTENER_TLS, || match env::var("TLS_ENABLED") {
        Ok(value) => parse_bool_env("TLS_ENABLED", &value),
        Err(env::VarError::NotPresent) => Ok(false),
        Err(env::VarError::NotUnicode(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "TLS_ENABLED must contain valid unicode data",
        )),
    })
}

pub fn listener_enabled() -> bool {
    enabled().expect("Listener TLS settings are validated at startup")
}

/// Whether clients reach this server over HTTPS: its own listener terminates
/// TLS, or a proxy in front of it does (the public base URL is `https://`).
/// Cookies get `Secure`, responses carry HSTS, and the request's own origin is
/// `https://` exactly when this holds. Read once: both inputs are startup
/// settings.
pub fn served_over_https() -> bool {
    static HTTPS: OnceLock<bool> = OnceLock::new();
    *HTTPS.get_or_init(|| https_from(listener_enabled(), &crate::conf::public_base_url()))
}

fn https_from(listener_tls: bool, public_base_url: &str) -> bool {
    listener_tls
        || public_base_url
            .get(.."https://".len())
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

fn required_path(env_key: &str) -> io::Result<String> {
    let value = env::var(env_key).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{env_key} must be set when TLS is enabled"),
        )
    })?;

    if !Path::new(&value).exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{env_key} points to a missing file: {value}"),
        ));
    }

    Ok(value)
}

pub fn rustls_server_config() -> io::Result<std::sync::Arc<rustls::ServerConfig>> {
    use rustls::ServerConfig;
    use rustls_pemfile::{certs, pkcs8_private_keys};
    use std::{fs::File, io::BufReader, sync::Arc};

    let key_path = required_path("TLS_KEY")?;
    let cert_path = required_path("TLS_CERT")?;

    let cert_chain = {
        let mut reader = BufReader::new(File::open(&cert_path)?);
        certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(io::Error::other)?
    };

    let private_key = {
        let mut reader = BufReader::new(File::open(&key_path)?);
        pkcs8_private_keys(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(io::Error::other)?
            .into_iter()
            .next()
            .map(rustls::pki_types::PrivateKeyDer::Pkcs8)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "no PKCS8 private key found in TLS_KEY",
                )
            })?
    };

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .map_err(io::Error::other)?;

    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::{cached_enabled, https_from, parse_bool_env};
    use std::{io, sync::OnceLock};

    #[test]
    fn listener_setting_is_read_once_and_invalid_values_do_not_become_false() {
        for value in [
            Ok(true),
            Ok(false),
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid TLS_ENABLED",
            )),
        ] {
            let cache = OnceLock::new();
            let first = cached_enabled(&cache, || value);
            let second =
                cached_enabled(&cache, || panic!("listener setting must not be read again"));
            match first {
                Ok(value) => assert_eq!(second.unwrap(), value),
                Err(error) => assert_eq!(second.unwrap_err().kind(), error.kind()),
            }
        }
    }

    #[test]
    fn https_comes_from_the_listener_or_the_public_url() {
        assert!(https_from(true, "http://localhost:5050"));
        assert!(https_from(false, "https://identity.example.com"));
        assert!(https_from(false, "HTTPS://identity.example.com"));
        assert!(!https_from(false, "http://localhost:5050"));
        assert!(!https_from(false, "https"));
    }

    #[test]
    fn parse_bool_env_accepts_true_values() {
        assert!(parse_bool_env("TLS_ENABLED", "true").unwrap());
        assert!(parse_bool_env("TLS_ENABLED", "On").unwrap());
        assert!(parse_bool_env("TLS_ENABLED", "1").unwrap());
    }

    #[test]
    fn parse_bool_env_accepts_false_values() {
        assert!(!parse_bool_env("TLS_ENABLED", "false").unwrap());
        assert!(!parse_bool_env("TLS_ENABLED", "off").unwrap());
        assert!(!parse_bool_env("TLS_ENABLED", "0").unwrap());
    }

    #[test]
    fn parse_bool_env_rejects_invalid_values() {
        assert!(parse_bool_env("TLS_ENABLED", "maybe").is_err());
    }
}
