mod error;
mod store;

pub use error::{StoreError, StoreResult};
pub use store::{AtomicStore, Store};

#[cfg(feature = "memory")]
mod memory;

#[cfg(feature = "memory")]
pub use memory::MemoryStore;

#[cfg(feature = "redis")]
mod redis;
#[cfg(feature = "redis")]
pub use redis::RedisScript;
#[cfg(feature = "redis")]
pub use redis::RedisStore;
