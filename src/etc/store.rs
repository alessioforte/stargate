#[cfg(feature = "memory")]
mod memory {
    use once_cell::sync::Lazy;
    use store::MemoryStore;

    pub static STORE: Lazy<MemoryStore> = Lazy::new(|| {
        let store = MemoryStore::new();
        store.run_cleaner(60);
        store
    });

    pub async fn init() -> &'static MemoryStore {
        Lazy::force(&STORE);
        tracing::info!("Memory store initialized");
        run_memory_backup();
        &STORE
    }

    pub fn use_store() -> &'static MemoryStore {
        &STORE
    }

    fn run_memory_backup() {
        let interval = std::time::Duration::from_secs(60);
        tokio::spawn(async move {
            loop {
                let store = use_store();
                let _ = store
                    .export_to_simple_json(".stargate/backup/memory.json")
                    .await;
                tokio::time::sleep(interval).await;
            }
        });
    }
}

#[cfg(feature = "redis")]
mod redis {
    use store::{RedisPoolConfig, RedisStore};
    use tokio::sync::OnceCell;

    static STORE: OnceCell<RedisStore> = OnceCell::const_new();

    pub async fn init() -> &'static RedisStore {
        STORE
            .get_or_init(|| async {
                let url = std::env::var("REDIS_URL")
                    .unwrap_or_else(|_| "redis://localhost:6379".to_string());

                let pool_size: usize = std::env::var("REDIS_POOL_SIZE")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(4);

                let config = RedisPoolConfig::new(pool_size);
                let store = RedisStore::with_config(&url, config)
                    .await
                    .expect("Failed to create Redis store");
                tracing::info!(
                    "Redis store initialized with pool of {} connection(s)",
                    store.pool_size()
                );
                store
            })
            .await
    }

    pub fn use_store() -> &'static RedisStore {
        STORE
            .get()
            .expect("Redis store not initialized. Call init() first")
    }
}

#[cfg(feature = "memory")]
pub use memory::init;
#[cfg(feature = "memory")]
pub use memory::use_store;

#[cfg(feature = "redis")]
pub use redis::init;
#[cfg(feature = "redis")]
pub use redis::use_store;
