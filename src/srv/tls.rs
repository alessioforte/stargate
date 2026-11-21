use std::{env, io, path::Path, sync::OnceLock};

pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Whether clients reach this request over HTTPS: its listener terminates
/// TLS, or a proxy in front does (the public base URL is `https://`). Cookies
/// get `Secure`, responses carry HSTS, and the request's own origin is
/// `https://` exactly when this holds.
pub fn request_https(listener_tls: bool) -> bool {
    listener_tls || public_url_https()
}

/// The public URL is a startup setting: its scheme is read once.
fn public_url_https() -> bool {
    static HTTPS: OnceLock<bool> = OnceLock::new();
    *HTTPS.get_or_init(|| over_https(false, &crate::conf::public_base_url()))
}

/// The same decision for an explicit listener state and URL, for callers that
/// do not read the process settings (tests and pure checks).
pub fn over_https(listener_tls: bool, public_base_url: &str) -> bool {
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
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject};
    use std::{fs::File, io::BufReader};

    let key_path = required_path("TLS_KEY")?;
    let cert_path = required_path("TLS_CERT")?;

    let cert_chain = CertificateDer::pem_reader_iter(BufReader::new(File::open(&cert_path)?))
        .collect::<Result<Vec<_>, _>>()
        .map_err(io::Error::other)?;

    let private_key =
        match PrivatePkcs8KeyDer::from_pem_reader(BufReader::new(File::open(&key_path)?)) {
            Ok(key) => PrivateKeyDer::Pkcs8(key),
            Err(rustls::pki_types::pem::Error::NoItemsFound) => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "no PKCS8 private key found in TLS_KEY",
                ));
            }
            Err(error) => return Err(io::Error::other(error)),
        };

    server_config(cert_chain, private_key)
}

/// Builds the TLS configuration every TLS listener shares. ALPN offers HTTP/2
/// and HTTP/1.1: without it a client can only negotiate HTTP/1.1, and gRPC
/// clients fail. The certificate does not change per listener.
pub(crate) fn server_config(
    cert_chain: Vec<rustls::pki_types::CertificateDer<'static>>,
    private_key: rustls::pki_types::PrivateKeyDer<'static>,
) -> io::Result<std::sync::Arc<rustls::ServerConfig>> {
    const ALPN_PROTOCOLS: [&[u8]; 2] = [b"h2", b"http/1.1"];

    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .map_err(io::Error::other)?;
    config.alpn_protocols = ALPN_PROTOCOLS
        .iter()
        .map(|protocol| protocol.to_vec())
        .collect();
    Ok(std::sync::Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::over_https;

    #[test]
    fn https_comes_from_the_listener_or_the_public_url() {
        assert!(over_https(true, "http://localhost:5050"));
        assert!(over_https(false, "https://identity.example.com"));
        assert!(over_https(false, "HTTPS://identity.example.com"));
        assert!(!over_https(false, "http://localhost:5050"));
        assert!(!over_https(false, "https"));
    }
}
