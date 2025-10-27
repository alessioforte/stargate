mod error;
mod store;

pub use error::{StoreError, StoreResult};
pub use store::{AtomicStore, DeserializeValue, SerializeValue, Store};

#[cfg(feature = "memory")]
pub mod memory;

#[cfg(feature = "redis")]
mod redis;
#[cfg(feature = "redis")]
pub use redis::RedisScript;
#[cfg(feature = "redis")]
pub use redis::RedisStore;
