use crate::error::{StoreError, StoreResult};
use crate::store::Store;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone)]
enum StoreValue {
    Simple(String, Option<DateTime<Utc>>),
    Hash(
        DashMap<String, (String, Option<DateTime<Utc>>)>,
        Option<DateTime<Utc>>,
    ),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub total_keys: usize,
    pub simple_keys: usize,
    pub hash_keys: usize,
    pub total_hash_fields: usize,
    pub estimated_memory_bytes: usize,
    pub operations: OperationStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for OperationStats {
    fn default() -> Self {
        Self {
            gets: 0,
            sets: 0,
            deletes: 0,
            exists_checks: 0,
            hash_gets: 0,
            hash_sets: 0,
            hash_deletes: 0,
            hash_exists_checks: 0,
            hash_getalls: 0,
            hash_keys_calls: 0,
            hash_vals_calls: 0,
            hash_len_calls: 0,
            cache_hits: 0,
            cache_misses: 0,
            expired_entries_cleaned: 0,
        }
    }
}

#[derive(Debug)]
struct AtomicOperationStats {
    gets: AtomicU64,
    sets: AtomicU64,
    deletes: AtomicU64,
    exists_checks: AtomicU64,
    hash_gets: AtomicU64,
    hash_sets: AtomicU64,
    hash_deletes: AtomicU64,
    hash_exists_checks: AtomicU64,
    hash_getalls: AtomicU64,
    hash_keys_calls: AtomicU64,
    hash_vals_calls: AtomicU64,
    hash_len_calls: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    expired_entries_cleaned: AtomicU64,
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
    fn to_operation_stats(&self) -> OperationStats {
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

pub struct MemoryStore {
    data: Arc<DashMap<String, StoreValue>>,
    stats: Arc<AtomicOperationStats>,
}

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        MemoryStore {
            data: Arc::clone(&self.data),
            stats: Arc::clone(&self.stats),
        }
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            data: Arc::new(DashMap::new()),
            stats: Arc::new(AtomicOperationStats::default()),
        }
    }

    fn is_expired(&self, exp: &Option<DateTime<Utc>>) -> bool {
        exp.map_or(false, |date| Utc::now() > date)
    }

    fn estimate_value_size(&self, value: &StoreValue) -> usize {
        match value {
            StoreValue::Simple(data, _) => {
                std::mem::size_of::<String>()
                    + data.len()
                    + std::mem::size_of::<Option<DateTime<Utc>>>()
            }
            StoreValue::Hash(hash_map, _) => {
                let base_size =
                    std::mem::size_of::<DashMap<String, (String, Option<DateTime<Utc>>)>>()
                        + std::mem::size_of::<Option<DateTime<Utc>>>();
                let entries_size: usize = hash_map
                    .iter()
                    .map(|entry| {
                        let (field, (value, _)) = entry.pair();
                        std::mem::size_of::<String>() * 2
                            + field.len()
                            + value.len()
                            + std::mem::size_of::<Option<DateTime<Utc>>>()
                    })
                    .sum();
                base_size + entries_size
            }
        }
    }

    pub fn get_storage_stats(&self) -> StorageStats {
        let mut simple_keys = 0;
        let mut hash_keys = 0;
        let mut total_hash_fields = 0;
        let mut estimated_memory_bytes = 0;

        for entry in self.data.iter() {
            let (key, value) = entry.pair();
            let key_size = std::mem::size_of::<String>() + key.len();

            match value {
                StoreValue::Simple(_, exp) => {
                    if !self.is_expired(exp) {
                        simple_keys += 1;
                    }
                }
                StoreValue::Hash(hash_map, exp) => {
                    if !self.is_expired(exp) {
                        hash_keys += 1;
                        let valid_fields: usize = hash_map
                            .iter()
                            .filter(|item| {
                                let (_, (_, field_exp)) = item.pair();
                                !self.is_expired(field_exp)
                            })
                            .count();
                        total_hash_fields += valid_fields;
                    }
                }
            }

            estimated_memory_bytes += key_size + self.estimate_value_size(value);
        }

        StorageStats {
            total_keys: simple_keys + hash_keys,
            simple_keys,
            hash_keys,
            total_hash_fields,
            estimated_memory_bytes,
            operations: self.stats.to_operation_stats(),
        }
    }

    pub fn reset_stats(&self) {
        self.stats.gets.store(0, Ordering::Relaxed);
        self.stats.sets.store(0, Ordering::Relaxed);
        self.stats.deletes.store(0, Ordering::Relaxed);
        self.stats.exists_checks.store(0, Ordering::Relaxed);
        self.stats.hash_gets.store(0, Ordering::Relaxed);
        self.stats.hash_sets.store(0, Ordering::Relaxed);
        self.stats.hash_deletes.store(0, Ordering::Relaxed);
        self.stats.hash_exists_checks.store(0, Ordering::Relaxed);
        self.stats.hash_getalls.store(0, Ordering::Relaxed);
        self.stats.hash_keys_calls.store(0, Ordering::Relaxed);
        self.stats.hash_vals_calls.store(0, Ordering::Relaxed);
        self.stats.hash_len_calls.store(0, Ordering::Relaxed);
        self.stats.cache_hits.store(0, Ordering::Relaxed);
        self.stats.cache_misses.store(0, Ordering::Relaxed);
        self.stats
            .expired_entries_cleaned
            .store(0, Ordering::Relaxed);
    }

    pub fn get_memory_usage_bytes(&self) -> usize {
        self.get_storage_stats().estimated_memory_bytes
    }

    pub fn get_total_keys(&self) -> usize {
        self.get_storage_stats().total_keys
    }

    pub fn get_cache_hit_ratio(&self) -> f64 {
        let hits = self.stats.cache_hits.load(Ordering::Relaxed) as f64;
        let misses = self.stats.cache_misses.load(Ordering::Relaxed) as f64;
        let total = hits + misses;

        if total == 0.0 { 0.0 } else { hits / total }
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn get<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
    ) -> StoreResult<Option<T>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.gets.fetch_add(1, Ordering::Relaxed);

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Simple(value, exp) => {
                    if self.is_expired(exp) {
                        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                        return Ok(None);
                    }
                    self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                    let deserialized = serde_json::from_str(value).map_err(|e| {
                        StoreError::DeserializationFailed(format!(
                            "Failed to deserialize value for key '{}': {}",
                            key, e
                        ))
                    })?;
                    Ok(Some(deserialized))
                }
                StoreValue::Hash(_, _) => {
                    self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                    Ok(None) // Cannot get hash as simple value
                }
            }
        } else {
            self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
    }

    async fn set<T: serde::Serialize + Send + Sync>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<()> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        self.stats.sets.fetch_add(1, Ordering::Relaxed);

        let data = serde_json::to_string(value).map_err(|e| {
            StoreError::SerializationFailed(format!(
                "Failed to serialize value for key '{}': {}",
                key, e
            ))
        })?;
        let exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));
        self.data
            .insert(key.to_string(), StoreValue::Simple(data, exp));
        Ok(())
    }

    async fn delete(&self, key: &str) -> StoreResult<bool> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.deletes.fetch_add(1, Ordering::Relaxed);
        Ok(self.data.remove(key).is_some())
    }

    async fn exists(&self, key: &str) -> StoreResult<bool> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.exists_checks.fetch_add(1, Ordering::Relaxed);

        if let Some(entry) = self.data.get(key) {
            let exp = match entry.value() {
                StoreValue::Simple(_, exp) => exp,
                StoreValue::Hash(_, exp) => exp,
            };
            Ok(!self.is_expired(exp))
        } else {
            Ok(false)
        }
    }

    async fn hset<T: serde::Serialize + Send + Sync>(
        &self,
        key: &str,
        field: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if field.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "Field cannot be empty".to_string(),
            ));
        }
        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        self.stats.hash_sets.fetch_add(1, Ordering::Relaxed);

        let serialized_value = serde_json::to_string(value).map_err(|e| {
            StoreError::SerializationFailed(format!(
                "Failed to serialize value for key '{}', field '{}': {}",
                key, field, e
            ))
        })?;
        let field_exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));

        if let Some(mut entry) = self.data.get_mut(key) {
            match entry.value_mut() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        return Ok(false);
                    }
                    let is_new = !hash_map.contains_key(field);
                    hash_map.insert(field.to_string(), (serialized_value, field_exp));
                    Ok(is_new)
                }
                StoreValue::Simple(_, _) => Ok(false), // Cannot set hash field on simple value
            }
        } else {
            let hash_map = DashMap::new();
            hash_map.insert(field.to_string(), (serialized_value, field_exp));
            self.data
                .insert(key.to_string(), StoreValue::Hash(hash_map, None));
            Ok(true)
        }
    }

    async fn hget<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
        field: &str,
    ) -> StoreResult<Option<T>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if field.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "Field cannot be empty".to_string(),
            ));
        }

        self.stats.hash_gets.fetch_add(1, Ordering::Relaxed);

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                        return Ok(None);
                    }
                    if let Some(value_entry) = hash_map.get(field) {
                        let (value, field_exp) = value_entry.value();
                        if self.is_expired(field_exp) {
                            self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                            return Ok(None);
                        }
                        self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                        let deserialized = serde_json::from_str(value).map_err(|e| {
                            StoreError::DeserializationFailed(format!(
                                "Failed to deserialize hash field '{}' from key '{}': {}",
                                field, key, e
                            ))
                        })?;
                        Ok(Some(deserialized))
                    } else {
                        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                        Ok(None)
                    }
                }
                StoreValue::Simple(_, _) => {
                    self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                    Ok(None) // Cannot get hash field from simple value
                }
            }
        } else {
            self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
    }

    async fn hdel(&self, key: &str, field: &str) -> StoreResult<bool> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if field.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "Field cannot be empty".to_string(),
            ));
        }

        self.stats.hash_deletes.fetch_add(1, Ordering::Relaxed);

        if let Some(mut entry) = self.data.get_mut(key) {
            match entry.value_mut() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        return Ok(false);
                    }
                    Ok(hash_map.remove(field).is_some())
                }
                StoreValue::Simple(_, _) => Ok(false), // Cannot delete hash field from simple value
            }
        } else {
            Ok(false)
        }
    }

    async fn hgetall<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
    ) -> StoreResult<HashMap<String, T>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.hash_getalls.fetch_add(1, Ordering::Relaxed);

        let mut result = HashMap::new();
        let mut deserialization_errors = Vec::new();

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if !self.is_expired(exp) {
                        for item in hash_map.iter() {
                            let (field, (value, field_exp)) = item.pair();
                            if !self.is_expired(field_exp) {
                                match serde_json::from_str::<T>(value) {
                                    Ok(deserialized) => {
                                        result.insert(field.clone(), deserialized);
                                    }
                                    Err(e) => {
                                        deserialization_errors
                                            .push(format!("field '{}': {}", field, e));
                                    }
                                }
                            }
                        }
                    }
                }
                StoreValue::Simple(_, _) => {} // Cannot get all hash fields from simple value
            }
        }

        if !deserialization_errors.is_empty() {
            return Err(StoreError::DeserializationFailed(format!(
                "Failed to deserialize some fields from key '{}': {}",
                key,
                deserialization_errors.join(", ")
            )));
        }

        Ok(result)
    }

    async fn hexists(&self, key: &str, field: &str) -> StoreResult<bool> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if field.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "Field cannot be empty".to_string(),
            ));
        }

        self.stats
            .hash_exists_checks
            .fetch_add(1, Ordering::Relaxed);

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        return Ok(false);
                    }
                    if let Some(entry) = hash_map.get(field) {
                        let (_, field_exp) = entry.value();
                        Ok(!self.is_expired(field_exp))
                    } else {
                        Ok(false)
                    }
                }
                StoreValue::Simple(_, _) => Ok(false), // Cannot check hash field existence in simple value
            }
        } else {
            Ok(false)
        }
    }

    async fn hkeys(&self, key: &str) -> StoreResult<Vec<String>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.hash_keys_calls.fetch_add(1, Ordering::Relaxed);

        let mut keys = Vec::new();

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if !self.is_expired(exp) {
                        for item in hash_map.iter() {
                            let (field, (_, field_exp)) = item.pair();
                            if !self.is_expired(field_exp) {
                                keys.push(field.clone());
                            }
                        }
                    }
                }
                StoreValue::Simple(_, _) => {} // Cannot get hash keys from simple value
            }
        }

        Ok(keys)
    }

    async fn hvals<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
    ) -> StoreResult<Vec<T>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.hash_vals_calls.fetch_add(1, Ordering::Relaxed);

        let mut values = Vec::new();
        let mut deserialization_errors = Vec::new();

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if !self.is_expired(exp) {
                        for (index, item) in hash_map.iter().enumerate() {
                            let (_, (value, field_exp)) = item.pair();
                            if !self.is_expired(field_exp) {
                                match serde_json::from_str::<T>(value) {
                                    Ok(deserialized) => {
                                        values.push(deserialized);
                                    }
                                    Err(e) => {
                                        deserialization_errors
                                            .push(format!("index {}: {}", index, e));
                                    }
                                }
                            }
                        }
                    }
                }
                StoreValue::Simple(_, _) => {} // Cannot get hash values from simple value
            }
        }

        if !deserialization_errors.is_empty() {
            return Err(StoreError::DeserializationFailed(format!(
                "Failed to deserialize some values from key '{}': {}",
                key,
                deserialization_errors.join(", ")
            )));
        }

        Ok(values)
    }

    async fn hlen(&self, key: &str) -> StoreResult<usize> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.hash_len_calls.fetch_add(1, Ordering::Relaxed);

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        Ok(0)
                    } else {
                        Ok(hash_map
                            .iter()
                            .filter(|item| {
                                let (_, (_, field_exp)) = item.pair();
                                !self.is_expired(field_exp)
                            })
                            .count())
                    }
                }
                StoreValue::Simple(_, _) => Ok(0), // Cannot get hash length from simple value
            }
        } else {
            Ok(0)
        }
    }
}

impl MemoryStore {
    pub fn run_cleaner(&self, interval: u64) {
        let data = self.data.clone();
        let stats = self.stats.clone();
        tokio::spawn(async move {
            log::info!(
                "MemoryStore cleaner started with interval: {} seconds",
                interval
            );
            let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(interval));
            loop {
                ticker.tick().await;
                let now = Utc::now();
                let mut removed = 0;
                data.retain(|_, value| {
                    match value {
                        StoreValue::Simple(_, exp) => {
                            let keep = exp.map_or(true, |exp| exp > now);
                            if !keep {
                                removed += 1;
                            }
                            keep
                        }
                        StoreValue::Hash(hash_map, exp) => {
                            // Clean expired fields within the hash
                            hash_map.retain(|_, (_, field_exp)| {
                                let keep = field_exp.map_or(true, |exp| exp > now);
                                if !keep {
                                    removed += 1;
                                }
                                keep
                            });

                            // Keep the hash if it's not expired and has fields
                            let keep_hash =
                                exp.map_or(true, |exp| exp > now) && !hash_map.is_empty();
                            if !keep_hash && hash_map.is_empty() {
                                removed += 1;
                            }
                            keep_hash
                        }
                    }
                });

                if removed > 0 {
                    stats
                        .expired_entries_cleaned
                        .fetch_add(removed, Ordering::Relaxed);
                    log::info!("MemoryStore cleaner removed {} expired entries", removed);
                }
            }
        });
    }
}

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
