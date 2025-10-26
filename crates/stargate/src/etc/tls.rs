use openssl::ssl::{SslAcceptor, SslAcceptorBuilder, SslFiletype, SslMethod};
use std::env;

pub fn builder() -> SslAcceptorBuilder {
    let key_file_path = env::var("TLS_KEY")
        .unwrap_or_else(|_| ".stargate/certificate/localhost/key.pem".to_string());
    let cert_file_path = env::var("TLS_CERT")
        .unwrap_or_else(|_| ".stargate/certificate/localhost/cert.pem".to_string());

    // load TLS keys
    // to create a self-signed temporary cert for testing:
    // `openssl req -x509 -newkey rsa:4096 -nodes -keyout key.pem -out cert.pem -days 365 -subj '/CN=localhost'`
    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    builder
        .set_private_key_file(key_file_path, SslFiletype::PEM)
        .unwrap();
    builder.set_certificate_chain_file(cert_file_path).unwrap();
    builder
}
