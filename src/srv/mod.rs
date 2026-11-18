pub mod banner;
pub mod health;
pub mod profile;
pub mod settings;
pub mod shutdown;
pub mod tls;

use crate::api::plane::PlaneSet;
use axum::{
    Router,
    extract::{Request, connect_info::ConnectInfo},
    response::Response,
};
use hyper::body::Incoming;
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use rustls::ServerConfig;
use std::{
    convert::Infallible,
    future::Future,
    io,
    net::SocketAddr,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
    sync::{Notify, Semaphore, watch},
    task::JoinSet,
};
use tokio_rustls::TlsAcceptor;
use tower::Service;
use tracing::{debug, info, warn};

/// Back off after a failed accept so a persistent error (an exhausted
/// file-descriptor table, for example) cannot spin the loop at full CPU.
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);

/// Per-connection limits, the same for every listener. The defaults bound slow
/// and hostile connections while leaving room for normal browser, API, and gRPC
/// traffic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpSettings {
    /// Time a client has to send the complete request headers (HTTP/1), and the
    /// time any connection has to send its first request before the protocol is
    /// recognized. It closes slowloris connections, mute connections, and a
    /// client that stops after the HTTP/2 preface.
    pub header_read_timeout: Duration,
    /// Largest request header list on HTTP/2. It covers the session cookie,
    /// bearer tokens, and a normal browser's headers with headroom, and bounds
    /// the header block a peer may make the server decode. HTTP/1 headers are
    /// bounded by hyper's read buffer and the read timeout.
    pub max_header_list_size: u32,
    /// Streams one HTTP/2 connection may run at once; further requests wait or
    /// open another connection.
    pub max_concurrent_streams: u32,
    /// HTTP/2 PING interval: silent peers are detected and their slot released.
    pub keep_alive_interval: Duration,
    /// How long a peer has to answer the keep-alive PING.
    pub keep_alive_timeout: Duration,
    /// Upper bound on the TLS handshake: a client that never completes it holds
    /// neither a worker nor a connection slot indefinitely.
    pub tls_handshake_timeout: Duration,
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            header_read_timeout: Duration::from_secs(10),
            max_header_list_size: 32 * 1024,
            max_concurrent_streams: 100,
            keep_alive_interval: Duration::from_secs(30),
            keep_alive_timeout: Duration::from_secs(10),
            tls_handshake_timeout: Duration::from_secs(10),
        }
    }
}

/// The listener that accepted a connection. Rules that differ per listener
/// (proxy trust, TLS, rate limits) read it instead of a process-wide setting.
#[derive(Debug)]
pub struct ListenerInfo {
    pub name: String,
    pub planes: PlaneSet,
    pub tls: bool,
}

/// Facts about the connection carrying a request: the peer address before any
/// proxy header, and the listener's identity. Every request carries exactly one
/// of these; `ConnectInfo` is only read where a test builds a request itself.
#[derive(Clone, Debug)]
pub struct ConnectionInfo {
    pub peer: SocketAddr,
    pub listener: Arc<ListenerInfo>,
}

impl ConnectionInfo {
    /// The connection facts of a request the server accepted.
    pub fn of<B>(request: &http::Request<B>) -> Option<&Self> {
        request.extensions().get::<Self>()
    }

    /// The peer address of a request: the connection the server accepted, or
    /// the `ConnectInfo` extension where a request is built without one.
    pub fn peer_of<B>(request: &http::Request<B>) -> Option<SocketAddr> {
        Self::of(request)
            .map(|connection| connection.peer)
            .or_else(|| {
                request
                    .extensions()
                    .get::<ConnectInfo<SocketAddr>>()
                    .map(|peer| peer.0)
            })
    }
}

/// One listener to open: its validated settings and the router it serves.
pub struct Listener {
    pub settings: settings::Listener,
    pub router: Router,
}

/// Serves the configured listeners until `shutdown` resolves, then stops
/// accepting on all of them and drains the open connections within `timeout`.
pub async fn serve(
    listeners: Vec<Listener>,
    tls: Option<Arc<ServerConfig>>,
    shutdown: impl Future<Output = ()>,
    timeout: Duration,
) -> io::Result<()> {
    let bound = bind(listeners, tls.as_ref()).await?;
    for listener in &bound {
        info!(
            listener = %listener.info.name,
            address = %listener.address,
            planes = %listener.info.planes,
            tls = listener.info.tls,
            "Serving"
        );
    }
    run(bound, HttpSettings::default(), shutdown, timeout).await;
    Ok(())
}

/// Opens every listener before serving any of them, so a busy port stops
/// startup instead of leaving a half-open process.
pub(crate) async fn bind(
    listeners: Vec<Listener>,
    tls: Option<&Arc<ServerConfig>>,
) -> io::Result<Vec<Bound<TcpListener>>> {
    let mut bound = Vec::with_capacity(listeners.len());
    for Listener { settings, router } in listeners {
        let socket = TcpListener::bind(settings.address).await.map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "listener `{}` cannot bind {}: {error}",
                    settings.name, settings.address
                ),
            )
        })?;
        let address = socket.local_addr()?;
        let tls_acceptor = if settings.tls {
            let config = tls.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "listener `{}` enables TLS but no certificate was loaded",
                        settings.name
                    ),
                )
            })?;
            Some(TlsAcceptor::from(Arc::clone(config)))
        } else {
            None
        };
        let info = Arc::new(ListenerInfo {
            name: settings.name,
            planes: settings.planes,
            tls: tls_acceptor.is_some(),
        });
        bound.push(Bound {
            info,
            address,
            tls: tls_acceptor,
            max_connections: settings.max_connections,
            socket,
            router,
        });
    }
    Ok(bound)
}

/// A bound listener: the socket and everything a connection needs. The source
/// is generic so a test can feed accept failures without a real socket.
pub(crate) struct Bound<S> {
    pub(crate) info: Arc<ListenerInfo>,
    pub(crate) address: SocketAddr,
    tls: Option<TlsAcceptor>,
    max_connections: usize,
    socket: S,
    router: Router,
}

/// Runs the listeners together: the first accept loop to finish (a panic, for
/// example) stops the others, and `shutdown` stops all of them.
pub(crate) async fn run<S>(
    bound: Vec<Bound<S>>,
    http: HttpSettings,
    shutdown: impl Future<Output = ()>,
    timeout: Duration,
) where
    S: Accept + Send + 'static,
{
    let (stop, _) = watch::channel(false);
    let mut listeners = JoinSet::new();
    for listener in bound {
        listeners.spawn(serve_listener(listener, http, stop.subscribe(), timeout));
    }
    tokio::select! {
        _ = shutdown => {}
        joined = listeners.join_next() => {
            if let Some(Err(error)) = joined {
                warn!(%error, "Listener task failed");
            }
        }
    }
    let _ = stop.send(true);
    while listeners.join_next().await.is_some() {}
}

/// The accept source of a listener.
pub(crate) trait Accept {
    fn accept(&self) -> impl Future<Output = io::Result<(TcpStream, SocketAddr)>> + Send;
}

impl Accept for TcpListener {
    async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        TcpListener::accept(self).await
    }
}

async fn serve_listener<S: Accept + Send + 'static>(
    listener: Bound<S>,
    http: HttpSettings,
    mut stop: watch::Receiver<bool>,
    timeout: Duration,
) {
    let Bound {
        info,
        tls,
        max_connections,
        socket,
        router,
        ..
    } = listener;
    let permits = Arc::new(Semaphore::new(max_connections));
    let mut connections = JoinSet::new();
    loop {
        while connections.try_join_next().is_some() {}
        // Wait for a free slot before accepting, so the kernel backlog holds
        // further connections instead of this process opening descriptors and
        // tasks without bound.
        let permit = tokio::select! {
            biased;
            _ = stopped(&mut stop) => break,
            permit = Arc::clone(&permits).acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => break,
            },
        };
        let accepted = tokio::select! {
            biased;
            _ = stopped(&mut stop) => break,
            accepted = socket.accept() => accepted,
        };
        let (tcp, peer) = match accepted {
            Ok(connection) => connection,
            Err(error) => {
                warn!(listener = %info.name, %error, "Accept failed");
                tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
                continue;
            }
        };
        if let Err(error) = tcp.set_nodelay(true) {
            debug!(listener = %info.name, %error, "Could not set TCP_NODELAY");
        }
        let service = Connection {
            router: router.clone(),
            peer,
            listener: Arc::clone(&info),
            started: Arc::new(Notify::new()),
        };
        let acceptor = tls.clone();
        let mut rx = stop.clone();
        connections.spawn(async move {
            let _permit = permit;
            if let Some(acceptor) = acceptor {
                let handshake = tokio::select! {
                    biased;
                    _ = stopped(&mut rx) => return,
                    handshake = tokio::time::timeout(
                        http.tls_handshake_timeout,
                        acceptor.accept(tcp),
                    ) => handshake,
                };
                match handshake {
                    Ok(Ok(tls)) => connection(tls, service, http, rx).await,
                    Ok(Err(error)) => debug!(%peer, %error, "TLS handshake failed"),
                    Err(_) => debug!(%peer, "TLS handshake timed out"),
                }
            } else {
                connection(tcp, service, http, rx).await;
            }
        });
    }
    drop(socket);
    let drain = async { while connections.join_next().await.is_some() {} };
    if tokio::time::timeout(timeout, drain).await.is_err() {
        info!(listener = %info.name, "Shutdown timeout reached, forcing close");
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    }
}

/// Resolves once the server is shutting down, including when shutdown was
/// requested before this call.
async fn stopped(stop: &mut watch::Receiver<bool>) -> bool {
    if *stop.borrow() {
        return true;
    }
    stop.changed().await.is_err() || *stop.borrow()
}

async fn connection<I>(
    io: I,
    service: Connection,
    http: HttpSettings,
    mut stop: watch::Receiver<bool>,
) where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let started = Arc::clone(&service.started);
    let mut builder = Builder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(http.header_read_timeout);
    builder
        .http2()
        .timer(TokioTimer::new())
        .keep_alive_interval(http.keep_alive_interval)
        .keep_alive_timeout(http.keep_alive_timeout)
        .max_concurrent_streams(http.max_concurrent_streams)
        .max_header_list_size(http.max_header_list_size);
    let connection =
        builder.serve_connection_with_upgrades(TokioIo::new(io), TowerToHyperService::new(service));
    tokio::pin!(connection);
    let mut draining = *stop.borrow();
    if draining {
        connection.as_mut().graceful_shutdown();
    }
    // hyper runs its header timer only once it recognizes the protocol, so a
    // connection that sends nothing, or stops after the HTTP/2 preface, would
    // hold its slot forever. The deadline ends when the first request reaches
    // the router; after that hyper's per-protocol timers take over.
    let first_request = started.notified();
    tokio::pin!(first_request);
    let deadline = tokio::time::sleep(http.header_read_timeout);
    tokio::pin!(deadline);
    let mut requested = false;
    loop {
        tokio::select! {
            result = &mut connection => {
                if let Err(error) = result { debug!(%error, "Connection closed"); }
                return;
            }
            changed = stop.changed(), if !draining => {
                if changed.is_err() { return; }
                draining = true;
                connection.as_mut().graceful_shutdown();
            }
            _ = &mut first_request, if !requested => requested = true,
            _ = &mut deadline, if !requested => {
                debug!("No request arrived before the header read timeout");
                return;
            }
        }
    }
}

/// The router wrapped once per listener; a connection only adds its own facts
/// to each request, so no route table is rebuilt per connection.
#[derive(Clone)]
struct Connection {
    router: Router,
    peer: SocketAddr,
    listener: Arc<ListenerInfo>,
    /// Signalled when the first request reaches the router: the first-request
    /// deadline stops there.
    started: Arc<Notify>,
}

impl Service<Request<Incoming>> for Connection {
    type Response = Response;
    type Error = Infallible;
    type Future = <Router as Service<Request<Incoming>>>::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <Router as Service<Request<Incoming>>>::poll_ready(&mut self.router, cx)
    }

    fn call(&mut self, mut request: Request<Incoming>) -> Self::Future {
        self.started.notify_one();
        request.extensions_mut().insert(ConnectionInfo {
            peer: self.peer,
            listener: Arc::clone(&self.listener),
        });
        self.router.call(request)
    }
}

#[cfg(test)]
mod tests;
