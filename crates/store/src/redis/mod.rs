#[allow(clippy::module_inception)]
mod redis;
mod scripts;

pub use redis::{
    Compression, CompressionConfig, RedisPoolConfig, RedisScript, RedisStore, StreamEntry,
};
