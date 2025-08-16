#[cfg(feature = "memory")]
mod memory {
    use once_cell::sync::Lazy;
    use store::MemoryStore;

    pub static STORE: Lazy<MemoryStore> = Lazy::new(|| {
        let store = MemoryStore::new();
        store.run_cleaner(60);
        store
    });

    pub fn init() {
        Lazy::force(&STORE);
        log::info!("Memory store initialized");
    }

    pub fn use_store() -> &'static MemoryStore {
        &STORE
    }
}

#[cfg(feature = "redis")]
mod redis {
    use once_cell::sync::Lazy;
    use store::RedisStore;

    pub static STORE: Lazy<RedisStore> = Lazy::new(|| {
        let url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());
        RedisStore::new(&url).expect("Failed to create Redis store")
    });

    pub fn init() {
        Lazy::force(&STORE);
        log::info!("Redis store initialized");
    }

    pub fn use_store() -> &'static RedisStore {
        &STORE
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
