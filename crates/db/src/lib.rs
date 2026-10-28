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

#[cfg(feature = "postgres")]
pub use sqlx::postgres::PgListener;

#[cfg(feature = "postgres")]
pub const CONFIG_CHANNEL: &str = "stargate_config";

/// Serializes the PostgreSQL tests that claim outbox rows, across every
/// module of this crate: cargo runs tests in parallel, and each claim takes
/// every pending row it can lock, so a concurrent claimer would steal (and
/// publish) another test's rows. Insert-only tests don't need it.
#[cfg(all(test, feature = "postgres"))]
pub(crate) static OUTBOX_CLAIM_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
