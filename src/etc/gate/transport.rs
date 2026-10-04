use gate::{cfg::RuntimeConfig, graph::ServiceNode};
use hyper_rustls::{ConfigBuilderExt, HttpsConnectorBuilder};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use rustls::{
    ClientConfig, ConfigBuilder, RootCertStore, WantsVerifier,
    pki_types::{CertificateDer, PrivateKeyDer},
};
use std::{collections::HashMap, fs::File, io::BufReader, sync::Arc, time::Duration};

type HyperConnector = hyper_rustls::HttpsConnector<HttpConnector>;
pub type HyperClient = Client<DeadlineConnector, axum::body::Body>;

#[derive(Clone)]
pub struct DeadlineConnector {
    inner: HyperConnector,
    timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
#[error("upstream connection deadline exceeded")]
struct ConnectionTimeout;

pub fn connection_timed_out(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut source = Some(error);
    while let Some(error) = source {
        if error.is::<ConnectionTimeout>() {
            return true;
        }
        source = error.source();
    }
    false
}

impl DeadlineConnector {
    pub(crate) fn new(inner: HyperConnector, timeout: Duration) -> Self {
        Self { inner, timeout }
    }
}

impl tower::Service<http::Uri> for DeadlineConnector {
    type Response = <HyperConnector as tower::Service<http::Uri>>::Response;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future =
        std::pin::Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, uri: http::Uri) -> Self::Future {
        let future = self.inner.call(uri);
        let timeout = self.timeout;
        Box::pin(async move {
            tokio::time::timeout(timeout, future)
                .await
                .map_err(|_| Box::new(ConnectionTimeout) as Self::Error)?
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransportPreparationError {
    #[error("unable to read {kind} file '{path}': {source}")]
    ReadFile {
        kind: &'static str,
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("unable to parse {kind} PEM file")]
    Pem {
        kind: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{kind} file must contain at least one certificate")]
    EmptyCertificates { kind: &'static str },
    #[error("invalid {kind} certificate: {source}")]
    Certificate {
        kind: &'static str,
        #[source]
        source: rustls::Error,
    },
    #[error("client key file must contain exactly one supported private key")]
    PrivateKeyCount,
    #[error("invalid client certificate/private key identity: {0}")]
    ClientIdentity(#[source] rustls::Error),
    #[error("unable to prepare TLS configuration: {0}")]
    Tls(#[source] rustls::Error),
    #[error("HTTP connector TLS configuration must have empty ALPN protocols")]
    Alpn,
    #[error(
        "http.upstreams.{upstream}.transport.connect_timeout must be a positive, representable duration"
    )]
    ConnectTimeout { upstream: String },
    #[error("http.services.{service} references an unavailable prepared upstream")]
    MissingUpstream { service: String },
}

pub(super) struct PreparedTransports {
    clients: HashMap<String, HyperClient>,
}

impl PreparedTransports {
    pub(super) fn get(&self, service: &str) -> Option<HyperClient> {
        self.clients.get(service).cloned()
    }
}

pub(super) fn prepare(
    config: &RuntimeConfig,
) -> Result<PreparedTransports, TransportPreparationError> {
    let default_tls = default_tls_config()?;
    let mtls = config.mtls().map(build_mtls).transpose()?;
    let mut upstream_clients = HashMap::new();
    for (name, upstream) in &config.compiled.http.upstreams {
        let timeout = connect_timeout(
            name,
            upstream
                .transport
                .as_ref()
                .and_then(|transport| transport.connect_timeout.as_deref()),
            config.compiled.runtime.connect_timeout,
        )?;
        let tls = if upstream
            .targets
            .iter()
            .any(|target| target.url.starts_with("https://"))
        {
            mtls.as_ref().unwrap_or(&default_tls)
        } else {
            &default_tls
        };
        upstream_clients.insert(name.clone(), build_hyper_client(timeout, tls)?);
    }
    let mut clients = HashMap::new();
    for (name, service) in &config.compiled.http.services {
        let ServiceNode::LoadBalancer { upstream, .. } = service else {
            continue;
        };
        let client = upstream_clients.get(upstream).ok_or_else(|| {
            TransportPreparationError::MissingUpstream {
                service: name.clone(),
            }
        })?;
        clients.insert(name.clone(), client.clone());
    }
    Ok(PreparedTransports { clients })
}

fn connect_timeout(
    upstream: &str,
    value: Option<&str>,
    default: Duration,
) -> Result<Duration, TransportPreparationError> {
    let Some(value) = value else {
        return Ok(default);
    };
    let duration = tools::parse_duration(value)
        .ok()
        .and_then(|duration| duration.to_std().ok());
    match duration {
        Some(duration)
            if !duration.is_zero() && std::time::Instant::now().checked_add(duration).is_some() =>
        {
            Ok(duration)
        }
        _ => Err(TransportPreparationError::ConnectTimeout {
            upstream: upstream.to_string(),
        }),
    }
}

fn tls_builder() -> Result<ConfigBuilder<ClientConfig, WantsVerifier>, TransportPreparationError> {
    ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(TransportPreparationError::Tls)
}

fn default_tls_config() -> Result<ClientConfig, TransportPreparationError> {
    Ok(tls_builder()?.with_webpki_roots().with_no_client_auth())
}

fn build_hyper_client(
    timeout: Duration,
    tls: &ClientConfig,
) -> Result<HyperClient, TransportPreparationError> {
    if !tls.alpn_protocols.is_empty() {
        return Err(TransportPreparationError::Alpn);
    }
    let mut connector = HttpConnector::new();
    connector.enforce_http(false);
    let https = HttpsConnectorBuilder::new()
        .with_tls_config(tls.clone())
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .wrap_connector(connector);
    Ok(Client::builder(TokioExecutor::new()).build(DeadlineConnector::new(https, timeout)))
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

fn build_mtls(mtls: &gate::cfg::MtlsConfig) -> Result<ClientConfig, TransportPreparationError> {
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

#[cfg(test)]
mod tests;
