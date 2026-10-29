use store::{Compression, CompressionConfig, RedisPoolConfig, RedisStore};
use tokio::sync::OnceCell;

static STORE: OnceCell<RedisStore> = OnceCell::const_new();

pub async fn init() -> anyhow::Result<&'static RedisStore> {
    STORE
        .get_or_try_init(|| async {
            let client = super::settings::client_from_env()?;

            let pool_size: usize = std::env::var("REDIS_POOL_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(4);

            let config = RedisPoolConfig::new(pool_size);
            #[cfg(feature = "redis-compression")]
            let com = Compression::Lz4;
            #[cfg(not(feature = "redis-compression"))]
            let com = Compression::None;
            let com_config = CompressionConfig::new(com);
            let store = RedisStore::with_client(client, config)
                .await?
                .with_compression(com_config);
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
