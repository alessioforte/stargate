mod config;
#[allow(clippy::module_inception)]
mod memory;
mod persistence;
mod stats;

pub use config::MemoryStoreConfig;
pub use memory::MemoryStore;
pub use stats::{
    AtomicOperationStats, MemoryEfficiencyStats, MemoryStorePerformanceMetrics, OperationStats,
    StorageStats,
};
