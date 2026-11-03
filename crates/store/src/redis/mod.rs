#[allow(clippy::module_inception)]
mod redis;
mod scripts;
mod streams;

pub use streams::{StreamGroupState, StreamId};

pub use redis::{
    Compression, CompressionConfig, RedisPoolConfig, RedisScript, RedisStore, StreamEntry,
};
