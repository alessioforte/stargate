use crate::{StoreError, StoreResult};
use redis::aio::{ConnectionLike, MultiplexedConnection};
use redis::{AsyncConnectionConfig, Cmd, Pipeline, RedisError, RedisFuture, Value};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Configuration for the Redis connection pool.
///
/// The pool maintains multiple multiplexed connections, each backed by its
/// own TCP socket to the Redis server. This enables truly parallel I/O across
/// connections rather than multiplexing everything through a single socket.
#[derive(Debug, Clone)]
pub struct RedisPoolConfig {
    /// Number of connections to maintain in the pool.
    ///
    /// Each connection is a separate multiplexed connection with its own TCP socket.
    /// A higher value allows more parallel I/O but consumes more file descriptors
    /// and Redis server connections.
    ///
    /// Defaults to `4`.
    pub pool_size: usize,

    /// Maximum time to wait for a command response before failing the
    /// operation. `None` disables the timeout (a hung Redis server will hang
    /// callers indefinitely — not recommended).
    ///
    /// Defaults to 5 seconds.
    pub response_timeout: Option<Duration>,

    /// Maximum time to wait when (re)establishing a TCP connection to the
    /// Redis server. `None` disables the timeout.
    ///
    /// Defaults to 5 seconds.
    pub connection_timeout: Option<Duration>,

    /// Called when a server rejects the required hash field expiry capability.
    pub on_incompatible_server: fn(),
}

impl Default for RedisPoolConfig {
    fn default() -> Self {
        Self {
            pool_size: 4,
            response_timeout: Some(Duration::from_secs(5)),
            connection_timeout: Some(Duration::from_secs(5)),
            on_incompatible_server: || {},
        }
    }
}

impl RedisPoolConfig {
    /// Create a new pool configuration with the given pool size and default
    /// timeouts (5s response, 5s connection).
    ///
    /// # Panics
    ///
    /// Will not panic here, but a pool size of 0 will be rejected at pool creation time.
    pub fn new(pool_size: usize) -> Self {
        Self {
            pool_size,
            ..Default::default()
        }
    }

    /// Set the per-command response timeout (`None` disables it).
    pub fn with_response_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.response_timeout = timeout;
        self
    }

    /// Set the connection-establishment timeout (`None` disables it).
    pub fn with_connection_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.connection_timeout = timeout;
        self
    }
}

tokio::task_local! {
    static REQUEST_BUDGET: Cell<Option<Duration>>;
}

/// Bound security-state operations, including connection establishment, to
/// the same request budget. Traffic limits and health checks pass `None`.
pub async fn with_request_budget<T>(
    budget: Option<Duration>,
    future: impl Future<Output = T>,
) -> T {
    REQUEST_BUDGET.scope(Cell::new(budget), future).await
}

#[derive(Clone)]
struct Connector {
    client: redis::Client,
    config: AsyncConnectionConfig,
    on_incompatible_server: fn(),
    last_incompatible_log: Arc<std::sync::Mutex<Option<Instant>>>,
}

impl Connector {
    async fn connect(&self) -> redis::RedisResult<MultiplexedConnection> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection_with_config(&self.config)
            .await?;
        // Test capability after authentication on every socket. COMMAND INFO
        // uses no application keys and works on both Redis and Valkey.
        let supported = redis::cmd("COMMAND")
            .arg("INFO")
            .arg("HEXPIRE")
            .query_async::<(Option<Value>,)>(&mut connection)
            .await;
        match supported {
            Ok((Some(_),)) => Ok(connection),
            Err(error) if error.is_io_error() => Err(error),
            result => {
                (self.on_incompatible_server)();
                let mut last = self
                    .last_incompatible_log
                    .lock()
                    .expect("Redis diagnostic lock");
                if last.is_none_or(|last| last.elapsed() >= Duration::from_secs(60)) {
                    *last = Some(Instant::now());
                    tracing::error!(
                        ?result,
                        "Redis capability check failed: HEXPIRE requires Redis 7.4 or Valkey 9.0 and COMMAND INFO permission"
                    );
                }
                Err(RedisError::from((
                    redis::ErrorKind::InvalidClientConfig,
                    "HEXPIRE requires Redis 7.4 or Valkey 9.0 and COMMAND INFO permission",
                )))
            }
        }
    }
}

#[derive(Default)]
struct Slot(Mutex<Option<Arc<MultiplexedConnection>>>);

impl Slot {
    async fn connect(
        &self,
        connector: &Connector,
    ) -> redis::RedisResult<Arc<MultiplexedConnection>> {
        let mut current = self.0.lock().await;
        if let Some(connection) = current.as_ref() {
            return Ok(connection.clone());
        }

        let connection = Arc::new(connector.connect().await?);
        *current = Some(connection.clone());
        Ok(connection)
    }

    async fn disconnect_if_closed<T>(
        &self,
        connection: &Arc<MultiplexedConnection>,
        result: &redis::RedisResult<T>,
    ) {
        if !result
            .as_ref()
            .is_err_and(|error| error.is_connection_dropped())
        {
            return;
        }
        let mut current = self.0.lock().await;
        if current
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, connection))
        {
            current.take();
        }
    }
}

/// A pool slot that reports application commands to the request's
/// round-trip counter. Validation is part of opening the connection.
#[derive(Clone)]
pub struct RedisConnection {
    connector: Arc<Connector>,
    slot: Arc<Slot>,
}

impl RedisConnection {
    async fn run<T>(
        &self,
        command: impl AsyncFnOnce(&mut MultiplexedConnection) -> redis::RedisResult<T>,
    ) -> redis::RedisResult<T> {
        let execute = async {
            let connection = self.slot.connect(&self.connector).await?;
            crate::observe::command_issued();
            let result = command(&mut (*connection).clone()).await;
            self.slot.disconnect_if_closed(&connection, &result).await;
            result
        };

        let Some(remaining) = REQUEST_BUDGET.try_with(Cell::get).ok().flatten() else {
            return execute.await;
        };
        if !remaining.is_zero() {
            let started = Instant::now();
            let result = tokio::time::timeout(remaining, execute).await;
            REQUEST_BUDGET.with(|budget| {
                budget.set(Some(remaining.saturating_sub(started.elapsed())));
            });
            if let Ok(result) = result {
                return result;
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "Redis request budget exhausted",
        )
        .into())
    }
}

impl ConnectionLike for RedisConnection {
    fn req_packed_command<'a>(&'a mut self, cmd: &'a Cmd) -> RedisFuture<'a, Value> {
        Box::pin(self.run(async |connection| connection.req_packed_command(cmd).await))
    }

    fn req_packed_commands<'a>(
        &'a mut self,
        pipeline: &'a Pipeline,
        offset: usize,
        count: usize,
    ) -> RedisFuture<'a, Vec<Value>> {
        Box::pin(self.run(async move |connection| {
            connection
                .req_packed_commands(pipeline, offset, count)
                .await
        }))
    }

    fn get_db(&self) -> i64 {
        self.connector
            .client
            .get_connection_info()
            .redis_settings()
            .db()
    }
}

/// Round-robin pool. Connections are opened on demand and writes are never
/// retried automatically when a socket is lost.
#[derive(Clone)]
pub(super) struct RedisPool {
    connections: Arc<[RedisConnection]>,
    next: Arc<AtomicUsize>,
}

impl RedisPool {
    pub(super) fn new(client: redis::Client, config: RedisPoolConfig) -> StoreResult<Self> {
        if config.pool_size == 0 {
            return Err(StoreError::InvalidInput(
                "Pool size must be at least 1".into(),
            ));
        }
        let settings = AsyncConnectionConfig::new()
            .set_connection_timeout(config.connection_timeout)
            .set_response_timeout(config.response_timeout);
        let connector = Arc::new(Connector {
            client,
            config: settings,
            on_incompatible_server: config.on_incompatible_server,
            last_incompatible_log: Arc::default(),
        });
        let connections = (0..config.pool_size)
            .map(|_| RedisConnection {
                connector: connector.clone(),
                slot: Arc::default(),
            })
            .collect();
        Ok(Self {
            connections,
            next: Arc::default(),
        })
    }

    pub(super) fn get(&self) -> RedisConnection {
        self.connections[self.next.fetch_add(1, Ordering::Relaxed) % self.size()].clone()
    }

    pub(super) fn replication_connection(&self, timeout: Duration) -> RedisConnection {
        let mut connector = (*self.connections[0].connector).clone();
        // Leave time to receive the WAIT reply after the server's timeout.
        connector.config = connector
            .config
            .set_response_timeout(Some(timeout + Duration::from_secs(5)));
        RedisConnection {
            connector: Arc::new(connector),
            slot: Arc::default(),
        }
    }

    pub(super) fn size(&self) -> usize {
        self.connections.len()
    }
}
