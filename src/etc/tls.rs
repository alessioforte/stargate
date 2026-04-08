use openssl::ssl::{SslAcceptor, SslAcceptorBuilder, SslFiletype, SslMethod};
use std::{env, io, path::Path};

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
