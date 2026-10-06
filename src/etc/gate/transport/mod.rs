mod connector;
mod tls;

#[cfg(test)]
pub(crate) use connector::DeadlineConnector;
use connector::build_hyper_client;
pub use connector::{HyperClient, connection_timed_out};
use gate::{
    cfg::{RuntimeConfig, UpstreamProtocol},
    graph::ServiceNode,
};
use rustls::ClientConfig;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tls::{build_mtls, default_tls_config};

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
    services: HashMap<String, PreparedTransport>,
}

#[derive(Clone)]
pub struct PreparedTransport {
    pub http: HyperClient,
    pub websocket_tls: Arc<ClientConfig>,
    pub connect_timeout: Duration,
    pub http1: bool,
}

impl PreparedTransports {
    pub(super) fn get(&self, service: &str) -> Option<&PreparedTransport> {
        self.services.get(service)
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
        let tls =
            if upstream.targets.iter().any(|target| {
                target.url.starts_with("https://") || target.url.starts_with("wss://")
            }) {
                mtls.as_ref().unwrap_or(&default_tls)
            } else {
                &default_tls
            };
        let protocols = upstream
            .transport
            .as_ref()
            .map_or(&[][..], |value| &value.protocols);
        let http1 = protocols.is_empty() || protocols.contains(&UpstreamProtocol::Http1);
        let http2 = protocols.is_empty() || protocols.contains(&UpstreamProtocol::Http2);
        let http = build_hyper_client(timeout, tls, http1, http2)?;
        let mut websocket_tls = tls.clone();
        // RFC 6455 section 4: this proxy uses HTTP/1.1 Upgrade, never h2 CONNECT.
        websocket_tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        upstream_clients.insert(
            name.clone(),
            PreparedTransport {
                http,
                websocket_tls: Arc::new(websocket_tls),
                connect_timeout: timeout,
                http1,
            },
        );
    }
    let mut services = HashMap::new();
    for (name, service) in &config.compiled.http.services {
        let ServiceNode::LoadBalancer { upstream, .. } = service else {
            continue;
        };
        let client = upstream_clients.get(upstream).ok_or_else(|| {
            TransportPreparationError::MissingUpstream {
                service: name.clone(),
            }
        })?;
        services.insert(name.clone(), client.clone());
    }
    Ok(PreparedTransports { services })
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

#[cfg(test)]
mod tests;
