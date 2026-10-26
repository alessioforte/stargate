//! Outbound HTTP to OAuth client endpoints: back-channel logout URIs and
//! `jwks_uri` documents. Short timeout, no redirects, and the system trust
//! roots plus an optional PEM bundle for clients behind a private CA.

use once_cell::sync::Lazy;
use std::time::Duration;

/// PEM bundle trusted in addition to the system roots.
const CA_FILE_ENV: &str = "OAUTH_CLIENT_CA_FILE";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    build_client().unwrap_or_else(|error| {
        tracing::error!("OAuth client endpoints: {error}; using the system trust roots");
        reqwest::Client::new()
    })
});

/// The shared client, built (and `OAUTH_CLIENT_CA_FILE` read) on first use.
pub fn client() -> &'static reqwest::Client {
    &CLIENT
}

fn build_client() -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        // Client endpoints answer directly; a redirect is not followed.
        .redirect(reqwest::redirect::Policy::none());
    if let Some(path) = std::env::var_os(CA_FILE_ENV).filter(|path| !path.is_empty()) {
        let pem = std::fs::read(&path)
            .map_err(|error| format!("{CA_FILE_ENV} {}: {error}", path.to_string_lossy()))?;
        for certificate in reqwest::Certificate::from_pem_bundle(&pem)
            .map_err(|error| format!("{CA_FILE_ENV}: {error}"))?
        {
            builder = builder.add_root_certificate(certificate);
        }
    }
    builder.build().map_err(|error| error.to_string())
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
