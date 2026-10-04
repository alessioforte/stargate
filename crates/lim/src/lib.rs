#[cfg(all(feature = "memory", feature = "redis"))]
compile_error!("lim must be built with exactly one backend: memory or redis");

#[cfg(not(any(feature = "memory", feature = "redis")))]
compile_error!("lim must be built with one backend: memory or redis");

#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
mod clock;
#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
mod decision;
#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
mod error;
// mod jitter;
#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
mod limiter;
#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
mod state;
#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
pub mod strategies;

#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
pub use clock::CachedClock;
#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
pub use decision::RateLimitDecision;
#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
pub use error::{RateLimitError, Result};
#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
pub use limiter::Limiter;
#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
pub use state::State;
#[cfg(any(
    all(feature = "memory", not(feature = "redis")),
    all(feature = "redis", not(feature = "memory")),
))]
pub use strategies::*;
