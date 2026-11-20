#[cfg(feature = "memory")]
mod memory;
#[cfg(feature = "redis")]
mod redis;
#[cfg(feature = "redis")]
mod settings;

#[cfg(feature = "memory")]
pub use memory::{init, save, use_store};
#[cfg(feature = "redis")]
pub use redis::{init, ping, save, use_store};

#[cfg(feature = "redis")]
pub(crate) use redis::with_security_budget;

#[cfg(feature = "memory")]
pub(crate) async fn with_security_budget<T>(
    _elapsed: std::time::Duration,
    future: impl Future<Output = T>,
) -> T {
    future.await
}

pub(crate) async fn delete_confirmed(key: &str) -> store::StoreResult<bool> {
    #[cfg(feature = "redis")]
    {
        use_store().delete_confirmed(key).await
    }
    #[cfg(feature = "memory")]
    {
        use store::Store;
        use_store().delete(key).await
    }
}

pub(crate) async fn set_confirmed<T: store::SerializeValue>(
    key: &str,
    value: &T,
    ttl: u64,
) -> store::StoreResult<()> {
    #[cfg(feature = "redis")]
    {
        use_store().set_confirmed(key, value, ttl).await
    }
    #[cfg(feature = "memory")]
    {
        use store::Store;
        use_store().set(key, value, Some(ttl)).await
    }
}
