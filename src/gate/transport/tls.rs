use super::TransportPreparationError;
use hyper_rustls::ConfigBuilderExt;
use rustls::{
    ClientConfig, ConfigBuilder, RootCertStore, WantsVerifier,
    pki_types::{CertificateDer, PrivateKeyDer},
};
use std::{fs::File, io::BufReader, sync::Arc};

fn tls_builder() -> Result<ConfigBuilder<ClientConfig, WantsVerifier>, TransportPreparationError> {
    ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(TransportPreparationError::Tls)
}

pub(super) fn default_tls_config() -> Result<ClientConfig, TransportPreparationError> {
    Ok(tls_builder()?.with_webpki_roots().with_no_client_auth())
}

fn reader(path: &str, kind: &'static str) -> Result<BufReader<File>, TransportPreparationError> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|source| TransportPreparationError::ReadFile {
            kind,
            path: path.to_string(),
            source,
        })
}

fn certificates(
    path: &str,
    kind: &'static str,
) -> Result<Vec<CertificateDer<'static>>, TransportPreparationError> {
    let certificates = rustls_pemfile::certs(&mut reader(path, kind)?)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| TransportPreparationError::Pem { kind, source })?;
    if certificates.is_empty() {
        return Err(TransportPreparationError::EmptyCertificates { kind });
    }
    Ok(certificates)
}

fn private_key(path: &str) -> Result<PrivateKeyDer<'static>, TransportPreparationError> {
    let mut key = None;
    for item in rustls_pemfile::read_all(&mut reader(path, "client key")?) {
        let item = item.map_err(|source| TransportPreparationError::Pem {
            kind: "client key",
            source,
        })?;
        let next = match item {
            rustls_pemfile::Item::Pkcs1Key(key) => key.into(),
            rustls_pemfile::Item::Pkcs8Key(key) => key.into(),
            rustls_pemfile::Item::Sec1Key(key) => key.into(),
            _ => continue,
        };
        if key.replace(next).is_some() {
            return Err(TransportPreparationError::PrivateKeyCount);
        }
    }
    key.ok_or(TransportPreparationError::PrivateKeyCount)
}

pub(super) fn build_mtls(
    mtls: &gate::cfg::MtlsConfig,
) -> Result<ClientConfig, TransportPreparationError> {
    let mut roots = RootCertStore::empty();
    for cert in certificates(&mtls.ca_cert_path, "CA")? {
        roots
            .add(cert)
            .map_err(|source| TransportPreparationError::Certificate { kind: "CA", source })?;
    }
    let chain = certificates(&mtls.client_cert_path, "client")?;
    for cert in &chain {
        rustls::server::ParsedCertificate::try_from(cert).map_err(|source| {
            TransportPreparationError::Certificate {
                kind: "client",
                source,
            }
        })?;
    }
    let key = private_key(&mtls.client_key_path)?;
    let certified_key =
        rustls::sign::CertifiedKey::from_der(chain, key, &rustls::crypto::ring::default_provider())
            .map_err(TransportPreparationError::ClientIdentity)?;
    // Require a confirmed match, including if a provider cannot compare SPKIs.
    // https://docs.rs/rustls/0.23.43/rustls/sign/struct.CertifiedKey.html#method.keys_match
    certified_key
        .keys_match()
        .map_err(TransportPreparationError::ClientIdentity)?;
    Ok(tls_builder()?
        .with_root_certificates(roots)
        .with_client_cert_resolver(Arc::new(rustls::sign::SingleCertAndKey::from(
            certified_key,
        ))))
}
