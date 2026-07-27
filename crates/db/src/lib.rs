#[cfg(all(feature = "sqlite", feature = "postgres"))]
compile_error!("db must be built with exactly one backend: sqlite or postgres");

#[cfg(not(any(feature = "sqlite", feature = "postgres")))]
compile_error!("db must be built with one backend: sqlite or postgres");

#[cfg(any(
    all(feature = "sqlite", not(feature = "postgres")),
    all(feature = "postgres", not(feature = "sqlite")),
))]
mod backend;
#[cfg(any(
    all(feature = "sqlite", not(feature = "postgres")),
    all(feature = "postgres", not(feature = "sqlite")),
))]
mod db;
#[cfg(any(
    all(feature = "sqlite", not(feature = "postgres")),
    all(feature = "postgres", not(feature = "sqlite")),
))]
pub mod ent;
#[cfg(any(
    all(feature = "sqlite", not(feature = "postgres")),
    all(feature = "postgres", not(feature = "sqlite")),
))]
pub mod repo;
#[cfg(any(
    all(feature = "sqlite", not(feature = "postgres")),
    all(feature = "postgres", not(feature = "sqlite")),
))]
pub mod service;
#[cfg(any(
    all(feature = "sqlite", not(feature = "postgres")),
    all(feature = "postgres", not(feature = "sqlite")),
))]
pub use db::{DbStore, InstanceBootstrapResult};
