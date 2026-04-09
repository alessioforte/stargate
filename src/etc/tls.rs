use openssl::ssl::{SslAcceptor, SslAcceptorBuilder, SslFiletype, SslMethod};
use std::{env, io, path::Path};

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

pub fn builder() -> io::Result<SslAcceptorBuilder> {
    let key_file_path = required_path("TLS_KEY")?;
    let cert_file_path = required_path("TLS_CERT")?;

    let mut builder =
        SslAcceptor::mozilla_intermediate(SslMethod::tls()).map_err(io::Error::other)?;
    builder
        .set_private_key_file(key_file_path, SslFiletype::PEM)
        .map_err(io::Error::other)?;
    builder
        .set_certificate_chain_file(cert_file_path)
        .map_err(io::Error::other)?;
    Ok(builder)
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
