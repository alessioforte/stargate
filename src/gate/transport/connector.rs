use super::TransportPreparationError;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::{TokioExecutor, TokioTimer},
};
use rustls::ClientConfig;
use std::time::Duration;

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

pub(super) enum ClientProtocol {
    Http { http1: bool, http2: bool },
    Grpc,
}

pub(super) fn build_hyper_client(
    timeout: Duration,
    tls: &ClientConfig,
    protocol: ClientProtocol,
) -> Result<HyperClient, TransportPreparationError> {
    if !tls.alpn_protocols.is_empty() {
        return Err(TransportPreparationError::Alpn);
    }
    let mut connector = HttpConnector::new();
    connector.enforce_http(false);
    connector.set_nodelay(true);
    let builder = HttpsConnectorBuilder::new()
        .with_tls_config(tls.clone())
        .https_or_http();
    let (http1, http2) = match protocol {
        ClientProtocol::Http { http1, http2 } => (http1, http2),
        ClientProtocol::Grpc => (false, true),
    };
    let https = match (http1, http2) {
        (true, true) => builder
            .enable_http1()
            .enable_http2()
            .wrap_connector(connector),
        (true, false) => builder.enable_http1().wrap_connector(connector),
        (false, _) => builder.enable_http2().wrap_connector(connector),
    };
    let mut client = Client::builder(TokioExecutor::new());
    // Keep connections warm between bursts while bounding idle sockets per origin.
    client
        .pool_timer(TokioTimer::new())
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(32);
    if http2 {
        // RFC 9113 §6.7. For HTTP, detect a silent peer before the 30s header
        // deadline. gRPC servers typically require 5 minutes between data-free PINGs.
        let (interval, ack_timeout, while_idle) = match protocol {
            ClientProtocol::Http { .. } => (20, 5, true),
            ClientProtocol::Grpc => (300, 20, false),
        };
        client
            .timer(TokioTimer::new())
            .http2_keep_alive_interval(Duration::from_secs(interval))
            .http2_keep_alive_timeout(Duration::from_secs(ack_timeout))
            .http2_keep_alive_while_idle(while_idle);
    }
    // The gateway executor owns replay eligibility and per-attempt signing.
    Ok(client
        .http2_only(!http1)
        .retry_canceled_requests(false)
        .build(DeadlineConnector::new(https, timeout)))
}
