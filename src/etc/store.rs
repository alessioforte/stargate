#[cfg(feature = "memory")]
mod memory {
    use once_cell::sync::Lazy;
    use std::sync::atomic::{AtomicBool, Ordering};
    use store::{MemoryStore, memory::MemoryStoreConfig};

    pub const MEMORY_BACKUP_PATH: &str = ".stargate/memory.bin";
    const DEFAULT_MEMORY_BACKUP_INTERVAL_SECS: u64 = 30;
    const DEFAULT_MEMORY_BACKUP_WRITE_THRESHOLD: u64 = 100;
    const MEMORY_BACKUP_CHECK_INTERVAL_SECS: u64 = 1;
    const DEFAULT_MAX_MEMORY_MB: usize = 100;

    static RESTORE_ATTEMPTED: AtomicBool = AtomicBool::new(false);
    static BACKUP_TASK_STARTED: AtomicBool = AtomicBool::new(false);

    pub static STORE: Lazy<MemoryStore> = Lazy::new(|| {
        let max_memory_mb: usize = std::env::var("STORE_MAX_MEMORY_MB")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_MEMORY_MB);

        let config = MemoryStoreConfig {
            max_memory_usage: max_memory_mb * 1024 * 1024,
            ..MemoryStoreConfig::default()
        };
        let store = MemoryStore::with_config(config);
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

        match MemoryStore::load_from_binary(MEMORY_BACKUP_PATH).await {
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

        let interval_secs = std::env::var("STORE_BACKUP_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v: &u64| *v > 0)
            .unwrap_or(DEFAULT_MEMORY_BACKUP_INTERVAL_SECS);
        let interval = std::time::Duration::from_secs(interval_secs);
        let write_threshold = std::env::var("STORE_BACKUP_WRITE_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .map(|v: u64| if v == 0 { None } else { Some(v) })
            .unwrap_or(Some(DEFAULT_MEMORY_BACKUP_WRITE_THRESHOLD));
        let check_interval = std::time::Duration::from_secs(MEMORY_BACKUP_CHECK_INTERVAL_SECS);
        tokio::spawn(async move {
            let mut last_persisted_revision = use_store().persistence_revision();
            let mut last_persisted_at = tokio::time::Instant::now();
            let mut ticker = tokio::time::interval(check_interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            tracing::info!(
                interval_secs,
                write_threshold,
                "Memory store backup task started"
            );
            loop {
                ticker.tick().await;

                let store = use_store();
                let current_revision = store.persistence_revision();
                if current_revision == last_persisted_revision {
                    continue;
                }

                let writes_since_last_persist =
                    current_revision.saturating_sub(last_persisted_revision);
                let interval_elapsed = last_persisted_at.elapsed() >= interval;
                let threshold_reached =
                    write_threshold.is_some_and(|threshold| writes_since_last_persist >= threshold);

                if !interval_elapsed && !threshold_reached {
                    continue;
                }

                if let Err(error) = store.save_to_binary(MEMORY_BACKUP_PATH).await {
                    tracing::warn!(
                        path = MEMORY_BACKUP_PATH,
                        %error,
                        "Failed to persist memory backup"
                    );
                } else {
                    last_persisted_revision = current_revision;
                    last_persisted_at = tokio::time::Instant::now();
                }
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
#[cfg(feature = "memory")]
pub async fn save() {
    let store = memory::use_store();
    if let Err(error) = store.save_to_binary(memory::MEMORY_BACKUP_PATH).await {
        tracing::warn!("Failed to save memory store on shutdown: {}", error);
    } else {
        tracing::info!("Memory store saved on shutdown");
    }
}

#[cfg(feature = "redis")]
pub use redis::init;
#[cfg(feature = "redis")]
pub use redis::use_store;
#[cfg(feature = "redis")]
pub async fn save() {
    // Redis is persistent; nothing to do on shutdown
    tracing::info!("Redis store: no shutdown save needed");
}
