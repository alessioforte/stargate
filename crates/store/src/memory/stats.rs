use super::MemoryStore;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub total_keys: usize,
    pub simple_keys: usize,
    pub hash_keys: usize,
    pub total_hash_fields: usize,
    pub estimated_memory_bytes: usize,
    pub operations: OperationStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperationStats {
    pub gets: u64,
    pub sets: u64,
    pub deletes: u64,
    pub exists_checks: u64,
    pub hash_gets: u64,
    pub hash_sets: u64,
    pub hash_deletes: u64,
    pub hash_exists_checks: u64,
    pub hash_getalls: u64,
    pub hash_keys_calls: u64,
    pub hash_vals_calls: u64,
    pub hash_len_calls: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub expired_entries_cleaned: u64,
}

#[derive(Debug)]
pub struct AtomicOperationStats {
    pub gets: AtomicU64,
    pub sets: AtomicU64,
    pub deletes: AtomicU64,
    pub exists_checks: AtomicU64,
    pub hash_gets: AtomicU64,
    pub hash_sets: AtomicU64,
    pub hash_deletes: AtomicU64,
    pub hash_exists_checks: AtomicU64,
    pub hash_getalls: AtomicU64,
    pub hash_keys_calls: AtomicU64,
    pub hash_vals_calls: AtomicU64,
    pub hash_len_calls: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub expired_entries_cleaned: AtomicU64,
}

impl Default for AtomicOperationStats {
    fn default() -> Self {
        Self {
            gets: AtomicU64::new(0),
            sets: AtomicU64::new(0),
            deletes: AtomicU64::new(0),
            exists_checks: AtomicU64::new(0),
            hash_gets: AtomicU64::new(0),
            hash_sets: AtomicU64::new(0),
            hash_deletes: AtomicU64::new(0),
            hash_exists_checks: AtomicU64::new(0),
            hash_getalls: AtomicU64::new(0),
            hash_keys_calls: AtomicU64::new(0),
            hash_vals_calls: AtomicU64::new(0),
            hash_len_calls: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            expired_entries_cleaned: AtomicU64::new(0),
        }
    }
}

impl AtomicOperationStats {
    pub fn to_operation_stats(&self) -> OperationStats {
        OperationStats {
            gets: self.gets.load(Ordering::Relaxed),
            sets: self.sets.load(Ordering::Relaxed),
            deletes: self.deletes.load(Ordering::Relaxed),
            exists_checks: self.exists_checks.load(Ordering::Relaxed),
            hash_gets: self.hash_gets.load(Ordering::Relaxed),
            hash_sets: self.hash_sets.load(Ordering::Relaxed),
            hash_deletes: self.hash_deletes.load(Ordering::Relaxed),
            hash_exists_checks: self.hash_exists_checks.load(Ordering::Relaxed),
            hash_getalls: self.hash_getalls.load(Ordering::Relaxed),
            hash_keys_calls: self.hash_keys_calls.load(Ordering::Relaxed),
            hash_vals_calls: self.hash_vals_calls.load(Ordering::Relaxed),
            hash_len_calls: self.hash_len_calls.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            expired_entries_cleaned: self.expired_entries_cleaned.load(Ordering::Relaxed),
        }
    }
}

/// Performance metrics specific to MemoryStore
#[derive(Debug, Clone)]
pub struct MemoryStorePerformanceMetrics {
    pub cache_hit_ratio: f64,
    pub total_operations: u64,
    pub total_hash_operations: u64,
    pub expired_cleanup_efficiency: f64,
    pub memory_usage_bytes: usize,
    pub total_keys: usize,
    pub average_key_size: usize,
}

/// Memory efficiency statistics for MemoryStore
#[derive(Debug, Clone)]
pub struct MemoryEfficiencyStats {
    pub total_keys: usize,
    pub simple_keys: usize,
    pub hash_keys: usize,
    pub atomic_keys: usize,
    pub total_hash_fields: usize,
    pub expired_keys: usize,
    pub fragmentation_ratio: f64,
}

#[allow(dead_code)]
pub fn print_stats(store: &MemoryStore) {
    let stats = store.get_storage_stats();
    let memory_mb = stats.estimated_memory_bytes as f64 / 1024.0 / 1024.0;
    let hit_ratio = store.get_cache_hit_ratio();

    println!("Storage Stats:");
    println!("  Total Keys:      {}", stats.total_keys);
    println!("  Simple Keys:     {}", stats.simple_keys);
    println!("  Hash Keys:       {}", stats.hash_keys);
    println!("  Hash Fields:     {}", stats.total_hash_fields);
    println!(
        "  Memory Usage:    {:.2} MB ({} bytes)",
        memory_mb, stats.estimated_memory_bytes
    );
    println!("  Cache Hit Ratio: {:.2}%", hit_ratio * 100.0);
    println!("Operation Stats:");
    println!("  Gets:            {}", stats.operations.gets);
    println!("  Sets:            {}", stats.operations.sets);
    println!("  Deletes:         {}", stats.operations.deletes);
    println!("  Exists Checks:   {}", stats.operations.exists_checks);
    println!("  Hash Gets:       {}", stats.operations.hash_gets);
    println!("  Hash Sets:       {}", stats.operations.hash_sets);
    println!("  Hash Deletes:    {}", stats.operations.hash_deletes);
    println!("  Hash Exists:     {}", stats.operations.hash_exists_checks);
    println!("  Hash GetAlls:    {}", stats.operations.hash_getalls);
    println!("  Hash Keys:       {}", stats.operations.hash_keys_calls);
    println!("  Hash Vals:       {}", stats.operations.hash_vals_calls);
    println!("  Hash Lens:       {}", stats.operations.hash_len_calls);
    println!("  Cache Hits:      {}", stats.operations.cache_hits);
    println!("  Cache Misses:    {}", stats.operations.cache_misses);
    println!(
        "  Expired Cleaned: {}",
        stats.operations.expired_entries_cleaned
    );
    println!();
}
