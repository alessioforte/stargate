#[cfg(feature = "memory")]
mod memory;
#[cfg(feature = "redis")]
mod redis;
#[cfg(feature = "redis")]
mod settings;

#[cfg(feature = "memory")]
pub use memory::{init, init_settings, save, use_store};
#[cfg(feature = "redis")]
pub use redis::{init, ping, save, use_store};
