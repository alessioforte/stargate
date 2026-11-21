//! Outbound HTTP to OAuth client endpoints: back-channel logout URIs and
//! `jwks_uri` documents. Short timeout, no redirects, and the system trust
//! roots plus an optional PEM bundle for clients behind a private CA.

use once_cell::sync::OnceCell;
use std::ffi::OsStr;
use std::time::Duration;

/// PEM bundle trusted in addition to the system roots.
const CA_FILE_ENV: &str = "OAUTH_CLIENT_CA_FILE";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

static CLIENT: OnceCell<reqwest::Client> = OnceCell::new();

/// Reads and validates `OAUTH_CLIENT_CA_FILE` once at startup: an unreadable
/// or invalid bundle stops the server instead of silently falling back to the
/// system roots on first use.
pub fn init() -> Result<(), String> {
    let client = build_client()?;
    let _ = CLIENT.set(client);
    Ok(())
}

/// The shared client. The server builds it at startup through [`init`]; tests
/// and standalone commands build it on first use.
pub fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        build_client().expect("OAuth client endpoint settings are validated at startup")
    })
}

fn build_client() -> Result<reqwest::Client, String> {
    let ca_file = std::env::var_os(CA_FILE_ENV).filter(|path| !path.is_empty());
    build_client_with(ca_file.as_deref())
}

fn build_client_with(ca_file: Option<&OsStr>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        // Client endpoints answer directly; a redirect is not followed.
        .redirect(reqwest::redirect::Policy::none());
    if let Some(path) = ca_file {
        let pem = std::fs::read(path)
            .map_err(|error| format!("{CA_FILE_ENV} {}: {error}", path.to_string_lossy()))?;
        let certificates = reqwest::Certificate::from_pem_bundle(&pem)
            .map_err(|error| format!("{CA_FILE_ENV}: {error}"))?;
        if certificates.is_empty() {
            return Err(format!("{CA_FILE_ENV} contains no PEM certificates"));
        }
        for certificate in certificates {
            builder = builder.add_root_certificate(certificate);
        }
    }
    builder.build().map_err(|error| {
        let cause = error_chain(&error);
        match ca_file {
            Some(path) => format!("{CA_FILE_ENV} {}: {cause}", path.to_string_lossy()),
            None => format!("failed to build the OAuth client endpoint HTTP client: {cause}"),
        }
    })
}

/// reqwest's message omits the cause (TLS, DNS, connect); include it.
pub fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temporary_pem(contents: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "stargate-oauth-client-ca-{}.pem",
            ulid::Ulid::generate()
        ));
        std::fs::write(&path, contents).expect("write temporary PEM");
        path
    }

    #[test]
    fn a_missing_ca_file_is_an_error_with_the_variable_name() {
        let path = std::env::temp_dir().join(format!(
            "stargate-missing-oauth-client-ca-{}.pem",
            ulid::Ulid::generate()
        ));
        let error = build_client_with(Some(path.as_os_str())).unwrap_err();
        assert!(error.contains(CA_FILE_ENV), "{error}");
    }

    #[test]
    fn an_invalid_or_empty_bundle_is_an_error() {
        for contents in [&b"not a certificate"[..], &b""[..]] {
            let path = temporary_pem(contents);
            let error = build_client_with(Some(path.as_os_str())).unwrap_err();
            assert!(error.contains(CA_FILE_ENV), "{error}");
            std::fs::remove_file(path).ok();
        }
    }

    #[test]
    fn a_pem_with_invalid_der_names_the_variable_and_the_cause() {
        let path = temporary_pem(b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n");
        let error = build_client_with(Some(path.as_os_str())).unwrap_err();
        let path = path.to_string_lossy().to_string();
        assert!(error.contains(CA_FILE_ENV), "{error}");
        assert!(error.contains(&path), "{error}");
        assert!(error.contains("BadEncoding"), "{error}");
        std::fs::remove_file(path).ok();
    }
}
