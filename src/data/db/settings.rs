//! PostgreSQL process settings, kept with the database subsystem.

use anyhow::{Context, Result, bail, ensure};
use db::{PgConnectOptions, PgSslMode};
use std::net::Ipv6Addr;

pub(super) struct PostgresSettings {
    host: String,
    port: u16,
    username: String,
    password: String,
    database: String,
    ssl_mode: PgSslMode,
    root_cert: Option<String>,
    pub(super) create_database: bool,
}

impl PostgresSettings {
    pub(super) fn from_env() -> Result<Self> {
        Self::from_lookup(|name| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(_) => bail!("{name} must contain valid Unicode"),
        })
    }

    fn from_lookup(mut lookup: impl FnMut(&str) -> Result<Option<String>>) -> Result<Self> {
        let endpoint = lookup("POSTGRES_ENDPOINT")?.unwrap_or_else(|| "localhost".into());
        let (host, port) = parse_endpoint(&endpoint)?;
        let username = lookup("POSTGRES_USERNAME")?.unwrap_or_else(|| "root".into());
        let password = lookup("POSTGRES_PASSWORD")?.unwrap_or_else(|| "root".into());
        let database = lookup("POSTGRES_DATABASE")?.unwrap_or_else(|| "stargate".into());
        let ssl_mode = match lookup("POSTGRES_SSLMODE")?.as_deref().unwrap_or("prefer") {
            "disable" => PgSslMode::Disable,
            "prefer" => PgSslMode::Prefer,
            "require" => PgSslMode::Require,
            "verify-ca" => PgSslMode::VerifyCa,
            "verify-full" => PgSslMode::VerifyFull,
            _ => bail!(
                "POSTGRES_SSLMODE must be disable, prefer, require, verify-ca, or verify-full"
            ),
        };
        let create_database = match lookup("POSTGRES_CREATE_DATABASE")?.as_deref() {
            None | Some("true") => true,
            Some("false") => false,
            _ => bail!("POSTGRES_CREATE_DATABASE must be true or false"),
        };

        let root_cert = lookup("POSTGRES_SSLROOTCERT")?;
        Ok(Self {
            host,
            port,
            username,
            password,
            database,
            ssl_mode,
            root_cert,
            create_database,
        })
    }

    pub(super) fn connect_options(&self) -> PgConnectOptions {
        // Keep sqlx defaults separate from pure parsing; never inspect .pgpass or build a URL.
        let mut options = PgConnectOptions::new_without_pgpass()
            .host(&self.host)
            .port(self.port)
            .username(&self.username)
            .password(&self.password)
            .database(&self.database)
            .ssl_mode(self.ssl_mode);
        if let Some(path) = &self.root_cert {
            options = options.ssl_root_cert(path);
        }
        options
    }
}

const ENDPOINT_ERROR: &str =
    "POSTGRES_ENDPOINT must be host, host:port, [ipv6], or [ipv6]:port (port 1-65535)";

fn parse_endpoint(value: &str) -> Result<(String, u16)> {
    if let Some(bracketed) = value.strip_prefix('[') {
        let (host, suffix) = bracketed.split_once(']').context(ENDPOINT_ERROR)?;
        host.parse::<Ipv6Addr>().context(ENDPOINT_ERROR)?;
        let port = if suffix.is_empty() {
            5432
        } else {
            parse_port(suffix.strip_prefix(':').context(ENDPOINT_ERROR)?)?
        };
        return Ok((host.to_string(), port));
    }

    let (host, port) = match value.split_once(':') {
        Some((host, port)) => (host, parse_port(port)?),
        None => (value, 5432),
    };
    let hostname = host.strip_suffix('.').unwrap_or(host);
    ensure!(
        !hostname.is_empty()
            && hostname.len() <= 253
            && hostname.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && label.as_bytes()[0].is_ascii_alphanumeric()
                    && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            }),
        ENDPOINT_ERROR
    );
    Ok((host.to_string(), port))
}

fn parse_port(value: &str) -> Result<u16> {
    ensure!(
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()),
        ENDPOINT_ERROR
    );
    let port = value.parse().context(ENDPOINT_ERROR)?;
    ensure!(port != 0, ENDPOINT_ERROR);
    Ok(port)
}

#[cfg(test)]
mod tests;
