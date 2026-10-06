use std::{env, io, path::Path};

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

pub fn enabled() -> io::Result<bool> {
    match env::var("TLS_ENABLED") {
        Ok(value) => parse_bool_env("TLS_ENABLED", &value),
        Err(env::VarError::NotPresent) => Ok(false),
        Err(env::VarError::NotUnicode(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "TLS_ENABLED must contain valid unicode data",
        )),
    }
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
    use super::parse_bool_env;

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
