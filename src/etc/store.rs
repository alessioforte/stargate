#[cfg(feature = "memory")]
mod memory {
    use once_cell::sync::Lazy;
    use std::sync::atomic::{AtomicBool, Ordering};
    use store::MemoryStore;

    const MEMORY_BACKUP_PATH: &str = ".stargate/backup/memory.json";
    const MEMORY_BACKUP_INTERVAL_SECS: u64 = 5;

    static RESTORE_ATTEMPTED: AtomicBool = AtomicBool::new(false);
    static BACKUP_TASK_STARTED: AtomicBool = AtomicBool::new(false);

    pub static STORE: Lazy<MemoryStore> = Lazy::new(|| {
        let store = MemoryStore::new();
        store.run_cleaner(60);
        store
    });

    pub async fn init() -> &'static MemoryStore {
        Lazy::force(&STORE);
        restore_memory_backup().await;
        tracing::info!("Memory store initialized");
        run_memory_backup();
        &STORE
    }

    pub fn use_store() -> &'static MemoryStore {
        &STORE
    }

    async fn restore_memory_backup() {
        if RESTORE_ATTEMPTED.swap(true, Ordering::AcqRel) {
            return;
        }

        match MemoryStore::load_from_json_with_fallback(MEMORY_BACKUP_PATH).await {
            Ok(restored_store) => {
                STORE.data.clear();
                for entry in restored_store.data.iter() {
                    STORE
                        .data
                        .insert(entry.key().clone(), entry.value().clone());
                }
                STORE.reset_stats();
                tracing::info!(
                    path = MEMORY_BACKUP_PATH,
                    keys = STORE.get_total_keys(),
                    "Memory store restored from backup"
                );
            }
            Err(error) => {
                tracing::info!(
                    path = MEMORY_BACKUP_PATH,
                    %error,
                    "No memory backup restored at startup"
                );
            }
        }
    }

    fn run_memory_backup() {
        if BACKUP_TASK_STARTED.swap(true, Ordering::AcqRel) {
            return;
        }

        let interval = std::time::Duration::from_secs(MEMORY_BACKUP_INTERVAL_SECS);
        tokio::spawn(async move {
            loop {
                let store = use_store();
                if let Err(error) = store.save_to_json(MEMORY_BACKUP_PATH).await {
                    tracing::warn!(
                        path = MEMORY_BACKUP_PATH,
                        %error,
                        "Failed to persist memory backup"
                    );
                }
                tokio::time::sleep(interval).await;
            }
        });
    }
}

#[cfg(feature = "redis")]
mod redis {
    use store::{Compression, CompressionConfig, RedisPoolConfig, RedisStore};
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
                #[cfg(feature = "memory-compression")]
                let com = Compression::Lz4;
                #[cfg(not(feature = "memory-compression"))]
                let com = Compression::None;
                let com_config = CompressionConfig::new(com);
                let store = RedisStore::with_config(&url, config)
                    .await
                    .expect("Failed to create Redis store")
                    .with_compression(com_config);
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
