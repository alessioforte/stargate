use store::{Compression, CompressionConfig, RedisPoolConfig, RedisStore};
use tokio::sync::OnceCell;

static STORE: OnceCell<RedisStore> = OnceCell::const_new();

pub async fn init() -> anyhow::Result<&'static RedisStore> {
    STORE
        .get_or_try_init(|| async {
            let client = super::settings::client_from_env()?;

            let pool_size = pool_size()?;

            let key_prefix =
                crate::conf::env::parse_or::<String>("REDIS_KEY_PREFIX", String::new())?;
            let config = RedisPoolConfig::new(pool_size);
            #[cfg(feature = "redis-compression")]
            let com = Compression::Lz4;
            #[cfg(not(feature = "redis-compression"))]
            let com = Compression::None;
            let com_config = CompressionConfig::new(com);
            let store = RedisStore::with_client(client, config)
                .await?
                .with_compression(com_config)
                .with_key_prefix(key_prefix);
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
    store::Store::ping(store).await?;
    Ok(())
}

pub async fn save() {
    // Redis is persistent; nothing to do on shutdown
    tracing::info!("Redis store: no shutdown save needed");
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
