//! Redis connection and TLS settings for the selected state-store backend.

use anyhow::{Context, Result, bail};
use redis::{ConnectionAddr, ConnectionInfo, IntoConnectionInfo, TlsCertificates};
use std::{io::Cursor, path::Path};

fn connection_info(
    url: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<ConnectionInfo> {
    let info = url
        .into_connection_info()
        .map_err(|_| anyhow::anyhow!("REDIS_URL is invalid"))?;
    if matches!(info.addr(), ConnectionAddr::TcpTls { insecure: true, .. }) {
        bail!("REDIS_URL must not disable TLS certificate verification");
    }
    let mut redis = info.redis_settings().clone();
    if (username.is_some() || password.is_some())
        && (redis.username().is_some() || redis.password().is_some())
    {
        bail!(
            "Redis credentials must be specified either in REDIS_URL or in REDIS_USERNAME/REDIS_PASSWORD, not both"
        );
    }
    if let Some(username) = username {
        redis = redis.set_username(username);
    }
    if let Some(password) = password {
        redis = redis.set_password(password);
    }
    Ok(info.set_redis_settings(redis))
}

fn read_ca(path: &Path) -> Result<Vec<u8>> {
    let pem = std::fs::read(path).context("cannot read REDIS_CA_FILE")?;
    let certs = rustls_pemfile::certs(&mut Cursor::new(&pem))
        .collect::<std::io::Result<Vec<_>>>()
        .context("REDIS_CA_FILE is not a valid PEM certificate bundle")?;
    if certs.is_empty() {
        bail!("REDIS_CA_FILE must contain at least one PEM certificate");
    }
    let mut roots = rustls::RootCertStore::empty();
    for cert in certs {
        roots
            .add(cert)
            .context("REDIS_CA_FILE contains an invalid certificate")?;
    }
    Ok(pem)
}

fn client(
    url: &str,
    username: Option<&str>,
    password: Option<&str>,
    ca: Option<&Path>,
) -> Result<redis::Client> {
    let info = connection_info(url, username, password)?;
    if ca.is_some() && !matches!(info.addr(), ConnectionAddr::TcpTls { .. }) {
        bail!("REDIS_CA_FILE requires a rediss:// REDIS_URL");
    }
    match ca {
        Some(path) => {
            let pem = read_ca(path)?;
            redis::Client::build_with_tls(
                info,
                TlsCertificates {
                    client_tls: None,
                    root_cert: Some(pem),
                },
            )
            .context("cannot configure Redis TLS with REDIS_CA_FILE")
        }
        None => redis::Client::open(info).context("cannot configure Redis client"),
    }
}

pub(super) fn client_from_env() -> Result<redis::Client> {
    fn lookup(name: &str) -> Result<Option<String>> {
        match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(error) => Err(error).with_context(|| format!("cannot read {name}")),
        }
    }
    let url = lookup("REDIS_URL")?.unwrap_or_else(|| "redis://localhost:6379".into());
    let username = lookup("REDIS_USERNAME")?;
    let password = lookup("REDIS_PASSWORD")?;
    let ca = lookup("REDIS_CA_FILE")?;
    client(
        &url,
        username.as_deref(),
        password.as_deref(),
        ca.as_deref().map(Path::new),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_ca_requires_a_tls_url_before_reading_the_file() {
        let error = client(
            "redis://localhost",
            None,
            None,
            Some(Path::new("/missing/redis-ca.pem")),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "REDIS_CA_FILE requires a rediss:// REDIS_URL"
        );
    }

    #[test]
    fn credentials_are_preserved_and_conflicting_sources_are_rejected() {
        let password = "@ : / ? # %";
        let info = connection_info(
            "rediss://redis.example:6379/2",
            Some("acl-user"),
            Some(password),
        )
        .unwrap();
        assert_eq!(info.redis_settings().username(), Some("acl-user"));
        assert_eq!(info.redis_settings().password(), Some(password));
        assert_eq!(info.redis_settings().db(), 2);
        for url in [
            "redis://user:secret@localhost",
            "redis://:secret@localhost",
            "redis://user@localhost",
        ] {
            assert!(connection_info(url, Some("other"), None).is_err());
            assert!(connection_info(url, None, Some("other")).is_err());
            assert!(connection_info(url, None, None).is_ok());
        }
        assert!(connection_info("rediss://localhost/#insecure", None, None).is_err());
    }

    #[test]
    fn missing_or_non_pem_ca_fails_without_connecting() {
        let path =
            std::env::temp_dir().join(format!("stargate-redis-ca-{}", ulid::Ulid::generate()));
        assert!(client("rediss://localhost", None, None, Some(&path)).is_err());
        std::fs::write(&path, b"not a PEM certificate").unwrap();
        assert!(client("rediss://localhost", None, None, Some(&path)).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
