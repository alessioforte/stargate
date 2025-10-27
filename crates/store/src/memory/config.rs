/// Configuration options for MemoryStore performance tuning
#[derive(Debug, Clone)]
pub struct MemoryStoreConfig {
    /// Initial capacity for the main data store
    pub initial_capacity: usize,
    /// Whether to enable time caching for expiration checks
    pub enable_time_caching: bool,
    /// Cleanup batch size for expired entries
    pub cleanup_batch_size: usize,
    /// Whether to pre-allocate HashMaps in hash operations
    pub enable_preallocation: bool,
    /// Maximum memory usage in bytes before triggering cleanup (0 = disabled)
    pub max_memory_usage: usize,
}

impl Default for MemoryStoreConfig {
    fn default() -> Self {
        Self {
            initial_capacity: 1000,
            enable_time_caching: true,
            cleanup_batch_size: 100,
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
///     cleanup_batch_size: 500,        // Clean 500 items per batch
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
///     cleanup_batch_size: 50,         // Smaller cleanup batches
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
/// // Manual cleanup with custom batch size
/// let cleaned = store.batch_cleanup_expired(Some(1000)).await;
/// println!("Cleaned {} expired entries", cleaned);
/// ```
impl MemoryStoreConfig {
    /// Configuration optimized for high-throughput applications
    pub fn high_performance() -> Self {
        Self {
            initial_capacity: 10000,
            enable_time_caching: true,
            cleanup_batch_size: 1000,
            enable_preallocation: true,
            max_memory_usage: 0, // No limit
        }
    }

    /// Configuration optimized for memory-constrained environments
    pub fn memory_efficient() -> Self {
        Self {
            initial_capacity: 100,
            enable_time_caching: true,
            cleanup_batch_size: 50,
            enable_preallocation: false,
            max_memory_usage: 50 * 1024 * 1024, // 50MB limit
        }
    }

    /// Configuration for development/testing with aggressive cleanup
    pub fn development() -> Self {
        Self {
            initial_capacity: 100,
            enable_time_caching: false, // Always get fresh time for testing
            cleanup_batch_size: 10,     // Small batches for predictable behavior
            enable_preallocation: true,
            max_memory_usage: 10 * 1024 * 1024, // 10MB limit
        }
    }
}
