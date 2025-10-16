#[cfg(all(feature = "redis", not(feature = "memory")))]
use store::RedisStore;
#[cfg(all(feature = "redis", not(feature = "memory")))]
pub type State = RedisStore;

#[cfg(all(feature = "memory", not(feature = "redis")))]
use store::MemoryStore;
#[cfg(all(feature = "memory", not(feature = "redis")))]
pub type State = MemoryStore;
