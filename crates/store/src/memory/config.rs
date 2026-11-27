/// Configuration options for MemoryStore performance tuning
#[derive(Debug, Clone)]
pub struct MemoryStoreConfig {
    /// Initial capacity for the main data store
    pub initial_capacity: usize,
    /// Whether to enable time caching for expiration checks
    pub enable_time_caching: bool,
    /// Whether to pre-allocate HashMaps in hash operations
    pub enable_preallocation: bool,
    /// Estimated bytes the store may hold before it refuses new entries
    /// (0 = unbounded). Nothing is ever evicted; see `MemoryStore::sweep`.
    pub max_memory_usage: usize,
}

impl Default for MemoryStoreConfig {
    fn default() -> Self {
        Self {
            initial_capacity: 1000,
            enable_time_caching: true,
            enable_preallocation: true,
            max_memory_usage: 0, // Disabled by default
        }
    }
}

/// # Usage Examples
///
/// ## Basic Usage with Default Configuration
/// ```rust,no_run
/// use store::memory::MemoryStore;
///
/// let store = MemoryStore::new(); // Uses default config
/// ```
///
/// ## High-Performance Configuration
/// ```rust,no_run
/// use store::memory::{MemoryStore, MemoryStoreConfig};
///
/// let config = MemoryStoreConfig {
///     initial_capacity: 10000,        // Pre-allocate for 10k items
///     enable_time_caching: true,      // Cache time calls (recommended)
///     enable_preallocation: true,     // Pre-allocate HashMaps (recommended)
///     max_memory_usage: 100 * 1024 * 1024, // 100MB memory limit
/// };
/// let store = MemoryStore::with_config(config);
/// ```
///
/// ## Memory-Constrained Configuration
/// ```rust,no_run
/// use store::memory::{MemoryStore, MemoryStoreConfig};
///
/// let config = MemoryStoreConfig {
///     initial_capacity: 100,          // Small initial size
///     enable_time_caching: true,      // Still beneficial
///     enable_preallocation: false,    // Save memory on allocations
///     max_memory_usage: 10 * 1024 * 1024, // 10MB limit
/// };
/// let store = MemoryStore::with_config(config);
/// ```
///
/// ## Performance Monitoring
/// ```ignore
/// // Get performance metrics
/// let metrics = store.get_performance_metrics();
/// println!("Cache hit ratio: {:.2}%", metrics.cache_hit_ratio * 100.0);
///
/// // Get optimization recommendations
/// let recommendations = store.get_optimization_recommendations();
/// for rec in recommendations {
///     println!("Tip: {}", rec);
/// }
///
/// // Remove expired entries now instead of waiting for the sweeper
/// let removed = store.sweep();
/// println!("Removed {} expired entries", removed);
/// ```
impl MemoryStoreConfig {
    /// Configuration optimized for high-throughput applications
    pub fn high_performance() -> Self {
        Self {
            initial_capacity: 10000,
            enable_time_caching: true,
            enable_preallocation: true,
            max_memory_usage: 0, // No limit
        }
    }

    /// Configuration optimized for memory-constrained environments
    pub fn memory_efficient() -> Self {
        Self {
            initial_capacity: 100,
            enable_time_caching: true,
            enable_preallocation: false,
            max_memory_usage: 50 * 1024 * 1024, // 50MB limit
        }
    }

    /// Configuration for development and testing
    pub fn development() -> Self {
        Self {
            initial_capacity: 100,
            enable_time_caching: false, // Always get fresh time for testing
            enable_preallocation: true,
            max_memory_usage: 10 * 1024 * 1024, // 10MB limit
        }
    }
}
