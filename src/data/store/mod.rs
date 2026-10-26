#[cfg(feature = "memory")]
mod memory;
#[cfg(feature = "redis")]
mod redis;

#[cfg(feature = "memory")]
pub use memory::{init, save, use_store};
#[cfg(feature = "redis")]
pub use redis::{init, ping, save, use_store};
