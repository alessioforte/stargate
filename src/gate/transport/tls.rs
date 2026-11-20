use super::TransportPreparationError;
use hyper_rustls::ConfigBuilderExt;
use rustls::{
    ClientConfig, ConfigBuilder, RootCertStore, WantsVerifier,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use sha2::{Digest, Sha256};
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
    let certificates = CertificateDer::pem_reader_iter(reader(path, kind)?)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| TransportPreparationError::Pem { kind, source })?;
    if certificates.is_empty() {
        return Err(TransportPreparationError::EmptyCertificates { kind });
    }
    Ok(certificates)
}

fn private_key(path: &str) -> Result<PrivateKeyDer<'static>, TransportPreparationError> {
    let keys = PrivateKeyDer::pem_reader_iter(reader(path, "client key")?)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| TransportPreparationError::Pem {
            kind: "client key",
            source,
        })?;

    let mut keys = keys.into_iter();
    match (keys.next(), keys.next()) {
        (Some(key), None) => Ok(key),
        _ => Err(TransportPreparationError::PrivateKeyCount),
    }
}

#[derive(Debug)]
pub(super) struct PreparedMtls {
    pub config: ClientConfig,
    pub fingerprint: [u8; 32],
}

pub(super) fn build_mtls(
    mtls: &gate::cfg::MtlsConfig,
) -> Result<PreparedMtls, TransportPreparationError> {
    let ca = certificates(&mtls.ca_cert_path, "CA")?;
    let mut roots = RootCertStore::empty();
    for cert in &ca {
        roots
            .add(cert.clone())
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
    // Include the material actually loaded, so rotating PEM files in place
    // creates fresh pools on the next publication.
    let mut fingerprint = Sha256::new();
    for certificates in [&ca, &chain] {
        fingerprint.update((certificates.len() as u64).to_be_bytes());
        for cert in certificates {
            fingerprint.update(Sha256::digest(cert.as_ref()));
        }
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
    let config = tls_builder()?
        .with_root_certificates(roots)
        .with_client_cert_resolver(Arc::new(rustls::sign::SingleCertAndKey::from(
            certified_key,
        )));
    Ok(PreparedMtls {
        config,
        fingerprint: fingerprint.finalize().into(),
    })
}
