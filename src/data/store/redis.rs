use std::{num::NonZeroU32, sync::OnceLock, time::Duration};
use store::{Compression, CompressionConfig, RedisPoolConfig, RedisStore};
use tokio::sync::OnceCell;

static SECURITY_TIMEOUT: OnceLock<Duration> = OnceLock::new();
static STORE: OnceCell<RedisStore> = OnceCell::const_new();

pub async fn init() -> anyhow::Result<&'static RedisStore> {
    STORE
        .get_or_try_init(|| async {
            let client = super::settings::client_from_env()?;

            let pool_size = pool_size()?;

            let key_prefix = key_prefix()?;
            let timeout = security_timeout()?;
            let replication = replication_config()?;
            let mut config = RedisPoolConfig::new(pool_size)
                .with_connection_timeout(Some(Duration::from_millis(250)));
            config.on_incompatible_server = telemetry::incompatible_server;
            #[cfg(feature = "redis-compression")]
            let com = Compression::Lz4;
            #[cfg(not(feature = "redis-compression"))]
            let com = Compression::None;
            let com_config = CompressionConfig::new(com);
            let mut store = RedisStore::with_client(client, config)?
                .with_compression(com_config)
                .with_key_prefix(key_prefix);
            if let Some(replication) = replication {
                store = store.with_replication(replication);
            }
            let _ = SECURITY_TIMEOUT.set(timeout);
            tracing::info!(
                "Redis store initialized with pool of {} connection(s)",
                store.pool_size()
            );
            Ok(store)
        })
        .await
}

pub fn use_store() -> &'static RedisStore {
    STORE
        .get()
        .expect("Redis store not initialized. Call init() first")
}

pub async fn ping() -> anyhow::Result<()> {
    let store = STORE
        .get()
        .ok_or_else(|| anyhow::anyhow!("Redis store not initialized"))?;
    // The diagnostics have their own budget and still report Redis failures
    // after a fail-open ingress check consumed the request's budget.
    store::with_request_budget(Some(Duration::from_secs(1)), store::Store::ping(store)).await?;
    Ok(())
}

pub async fn save() {
    // Redis is persistent; nothing to do on shutdown
    tracing::info!("Redis store: no shutdown save needed");
}

fn security_timeout() -> Result<Duration, crate::conf::env::EnvError> {
    crate::conf::env::parse_or("REDIS_SECURITY_TIMEOUT_MS", NonZeroU32::new(1000).unwrap())
        .map(|ms| Duration::from_millis(ms.get().into()))
}

fn replication_config() -> Result<Option<store::RedisReplicationConfig>, crate::conf::env::EnvError>
{
    use crate::conf::env::parse_or;

    let replicas = parse_or("REDIS_WAIT_REPLICAS", 0u32)?;
    let timeout_ms = parse_or("REDIS_WAIT_TIMEOUT_MS", NonZeroU32::new(250).unwrap())?;
    let fail_on_timeout = parse_or("REDIS_WAIT_FAIL_ON_TIMEOUT", true)?;
    Ok(
        NonZeroU32::new(replicas).map(|replicas| store::RedisReplicationConfig {
            replicas,
            timeout_ms,
            fail_on_timeout,
            on_timeout: telemetry::replication_timeout,
        }),
    )
}

pub(crate) async fn with_security_budget<T>(
    elapsed: Duration,
    future: impl Future<Output = T>,
) -> T {
    let budget = SECURITY_TIMEOUT
        .get()
        .copied()
        .unwrap_or(Duration::from_secs(1));
    store::with_request_budget(Some(budget.saturating_sub(elapsed)), future).await
}

mod telemetry {
    use opentelemetry::{KeyValue, metrics::Counter};
    use std::sync::LazyLock;

    static INCOMPATIBLE: LazyLock<Counter<u64>> = LazyLock::new(|| {
        crate::obs::telemetry::meter()
            .u64_counter("stargate.store.redis.incompatible_connections")
            .with_description("Redis connections rejected for lacking HEXPIRE capability")
            .build()
    });
    static REPLICATION_TIMEOUT: LazyLock<Counter<u64>> = LazyLock::new(|| {
        crate::obs::telemetry::meter()
            .u64_counter("stargate.store.redis.replication_timeouts")
            .with_description("Confirmed Redis writes whose WAIT did not reach the replica target")
            .build()
    });

    pub(super) fn incompatible_server() {
        INCOMPATIBLE.add(1, &[]);
    }

    pub(super) fn replication_timeout(fail: bool) {
        let outcome = if fail { "error" } else { "warn" };
        REPLICATION_TIMEOUT.add(1, &[KeyValue::new("stargate.outcome", outcome)]);
    }
}

/// The operator prefix sits before the `stargate:v1:` schema root. Braces are
/// rejected: Redis hashes the first `{...}` pair of the physical key, so a
/// prefix such as `{stargate}:` would put every key in one Cluster slot.
fn key_prefix() -> Result<String, crate::conf::env::EnvError> {
    let prefix = crate::conf::env::parse_or::<String>("REDIS_KEY_PREFIX", String::new())?;
    if prefix.contains(['{', '}']) {
        return Err(crate::conf::env::EnvError::Parse {
            name: "REDIS_KEY_PREFIX".to_string(),
            value: prefix,
            message: "must not contain '{' or '}': a hash tag in the operator prefix would put every state key in one Redis Cluster slot".to_string(),
        });
    }
    Ok(prefix)
}

fn pool_size() -> Result<usize, crate::conf::env::EnvError> {
    crate::conf::env::parse_or::<std::num::NonZeroUsize>(
        "REDIS_POOL_SIZE",
        std::num::NonZeroUsize::new(4).unwrap(),
    )
    .map(std::num::NonZeroUsize::get)
}

#[cfg(test)]
mod pool_tests {
    #[test]
    fn invalid_pool_sizes_fail_with_the_variable_name() {
        const CHILD: &str = "STARGATE_TEST_REDIS_POOL_SIZE";
        if let Ok(expected) = std::env::var(CHILD) {
            match super::pool_size() {
                Ok(actual) => assert_eq!(actual.to_string(), expected),
                Err(error) => {
                    assert_eq!(expected, "error");
                    assert!(error.to_string().contains("REDIS_POOL_SIZE"));
                }
            }
            return;
        }
        for (value, expected) in [
            (None, "4"),
            (Some("3"), "3"),
            (Some("0"), "error"),
            (Some("-1"), "error"),
            (Some(""), "error"),
            (Some("invalid"), "error"),
        ] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.args(["--exact", "data::store::redis::pool_tests::invalid_pool_sizes_fail_with_the_variable_name", "--nocapture"]).env(CHILD, expected);
            if let Some(value) = value {
                child.env("REDIS_POOL_SIZE", value);
            } else {
                child.env_remove("REDIS_POOL_SIZE");
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{value:?}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[cfg(test)]
mod prefix_tests {
    #[test]
    fn key_prefix_rejects_hash_tags() {
        const CHILD: &str = "STARGATE_TEST_REDIS_KEY_PREFIX";
        if let Ok(expected) = std::env::var(CHILD) {
            match super::key_prefix() {
                Ok(actual) => assert_eq!(actual, expected),
                Err(error) => {
                    assert_eq!(expected, "error");
                    assert!(error.to_string().contains("REDIS_KEY_PREFIX"));
                }
            }
            return;
        }
        for (value, expected) in [
            (None, ""),
            (Some("tenant:"), "tenant:"),
            (Some("{stargate}:"), "error"),
            (Some("tenant:{tag}:"), "error"),
            (Some("tenant}:"), "error"),
        ] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "data::store::redis::prefix_tests::key_prefix_rejects_hash_tags",
                    "--nocapture",
                ])
                .env(CHILD, expected);
            if let Some(value) = value {
                child.env("REDIS_KEY_PREFIX", value);
            } else {
                child.env_remove("REDIS_KEY_PREFIX");
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{value:?}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
