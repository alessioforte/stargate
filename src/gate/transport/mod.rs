mod connector;
mod tls;

#[cfg(test)]
pub(crate) use connector::DeadlineConnector;
use connector::build_hyper_client;
pub use connector::{HyperClient, connection_timed_out};
use gate::{
    cfg::{ProbeProtocol, RuntimeConfig, UpstreamProtocol},
    graph::ServiceNode,
};
use rustls::ClientConfig;
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
    time::Duration,
};
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
        source: rustls::pki_types::pem::Error,
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
    #[error(
        "http.upstreams.{upstream}.load_balancer.liveness_probe.timeout must be a positive, representable duration"
    )]
    ProbeTimeout { upstream: String },
    #[error("unable to prepare health probe client for upstream {upstream}: {source}")]
    ProbeClient {
        upstream: String,
        #[source]
        source: reqwest::Error,
    },
}

pub(super) struct PreparedTransports {
    services: HashMap<String, PreparedTransport>,
}

#[derive(Clone)]
pub struct PreparedTransport {
    pub http: HyperClient,
    pub grpc_http: Option<HyperClient>,
    pub probe_client: Option<reqwest::Client>,
    pub websocket_tls: Arc<ClientConfig>,
    connection: Arc<ConnectionSettings>,
    probe: Option<ProbeSettings>,
}

#[derive(PartialEq, Eq)]
struct ConnectionSettings {
    upstream: String,
    targets: BTreeSet<String>,
    mtls_fingerprint: Option<[u8; 32]>,
    connect_timeout: Duration,
    http1: bool,
    http2: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ProbeSettings {
    timeout: Duration,
    http2_only: bool,
}

impl PreparedTransport {
    pub fn connect_timeout(&self) -> Duration {
        self.connection.connect_timeout
    }

    pub fn http1(&self) -> bool {
        self.connection.http1
    }
}

impl PreparedTransports {
    pub(super) fn get(&self, service: &str) -> Option<&PreparedTransport> {
        self.services.get(service)
    }

    pub(super) fn reuse_from(&mut self, previous: &Self) {
        // Service aliases may change while the upstream keeps its connections.
        let by_upstream: HashMap<_, _> = previous
            .services
            .values()
            .map(|transport| (transport.connection.upstream.as_str(), transport))
            .collect();
        for transport in self.services.values_mut() {
            let Some(active) = by_upstream
                .get(transport.connection.upstream.as_str())
                .filter(|active| transport.connection == active.connection)
            else {
                continue;
            };
            transport.http = active.http.clone();
            transport.grpc_http = active.grpc_http.clone();
            transport.websocket_tls = active.websocket_tls.clone();
            if transport.probe == active.probe {
                transport.probe_client = active.probe_client.clone();
            }
        }
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
        let upstream_mtls = mtls.as_ref().filter(|_| {
            upstream.targets.iter().any(|target| {
                target.url.starts_with("https://") || target.url.starts_with("wss://")
            })
        });
        let tls = upstream_mtls.map_or(&default_tls, |mtls| &mtls.config);
        let protocols = upstream
            .transport
            .as_ref()
            .map_or(&[][..], |value| &value.protocols);
        let http1 = protocols.is_empty() || protocols.contains(&UpstreamProtocol::Http1);
        let http2 = protocols.is_empty() || protocols.contains(&UpstreamProtocol::Http2);
        let http = build_hyper_client(timeout, tls, http1, http2)?;
        let grpc_http = if http2 {
            Some(if http1 {
                build_hyper_client(timeout, tls, false, true)?
            } else {
                http.clone()
            })
        } else {
            None
        };
        let probe = upstream
            .load_balancer
            .liveness_probe
            .as_ref()
            .map(|probe| {
                probe
                    .timeout()
                    .map(|timeout| ProbeSettings {
                        timeout,
                        http2_only: probe.protocol == ProbeProtocol::Grpc || !http1,
                    })
                    .map_err(|_| TransportPreparationError::ProbeTimeout {
                        upstream: name.clone(),
                    })
            })
            .transpose()?;
        let probe_client = probe
            .map(|probe| build_probe_client(name, tls, probe))
            .transpose()?;
        let mut websocket_tls = tls.clone();
        // RFC 6455 section 4: this proxy uses HTTP/1.1 Upgrade, never h2 CONNECT.
        websocket_tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        upstream_clients.insert(
            name.clone(),
            PreparedTransport {
                http,
                grpc_http,
                probe_client,
                websocket_tls: Arc::new(websocket_tls),
                connection: Arc::new(ConnectionSettings {
                    upstream: name.clone(),
                    targets: upstream
                        .targets
                        .iter()
                        .map(|target| target.url.clone())
                        .collect(),
                    mtls_fingerprint: upstream_mtls.map(|mtls| mtls.fingerprint),
                    connect_timeout: timeout,
                    http1,
                    http2,
                }),
                probe,
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

fn build_probe_client(
    upstream: &str,
    tls: &ClientConfig,
    settings: ProbeSettings,
) -> Result<reqwest::Client, TransportPreparationError> {
    let mut tls = tls.clone();
    if settings.http2_only {
        tls.alpn_protocols = vec![b"h2".to_vec()];
    }
    let mut client = reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .connect_timeout(settings.timeout)
        .timeout(settings.timeout);
    if settings.http2_only {
        client = client.http2_prior_knowledge();
    }
    client
        .build()
        .map_err(|source| TransportPreparationError::ProbeClient {
            upstream: upstream.to_string(),
            source,
        })
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
