use super::config::MemoryStoreConfig;
use super::stats::{
    AtomicOperationStats, MemoryEfficiencyStats, MemoryStorePerformanceMetrics, StorageStats,
};
use crate::error::{StoreError, StoreResult};
use crate::store::{AtomicStore, DeserializeValue, SerializeValue, Store};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use portable_atomic::AtomicI64;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub type Data = Vec<u8>;

pub const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

#[derive(Debug, Clone)]
pub enum StoreValue {
    Simple(Data, Option<DateTime<Utc>>),
    Hash(
        DashMap<String, (Data, Option<DateTime<Utc>>)>,
        Option<DateTime<Utc>>,
    ),
    AtomicI64(Arc<AtomicI64>, Option<DateTime<Utc>>),
}

pub struct MemoryStore {
    pub data: Arc<DashMap<String, StoreValue>>,
    pub stats: Arc<AtomicOperationStats>,
    config: MemoryStoreConfig,
}

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        MemoryStore {
            data: Arc::clone(&self.data),
            stats: Arc::clone(&self.stats),
            config: self.config.clone(),
        }
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::with_config(MemoryStoreConfig::default())
    }

    pub fn with_config(config: MemoryStoreConfig) -> Self {
        let data = if config.initial_capacity > 0 {
            Arc::new(DashMap::with_capacity(config.initial_capacity))
        } else {
            Arc::new(DashMap::new())
        };

        Self {
            data,
            stats: Arc::new(AtomicOperationStats::default()),
            config,
        }
    }

    pub fn get_config(&self) -> &MemoryStoreConfig {
        &self.config
    }

    /// Get cached current time to reduce syscalls
    fn get_cached_now(&self) -> DateTime<Utc> {
        if !self.config.enable_time_caching {
            return Utc::now();
        }

        thread_local! {
            static CACHED_TIME: std::cell::Cell<Option<(DateTime<Utc>, std::time::Instant)>> = std::cell::Cell::new(None);
        }

        CACHED_TIME.with(|cached| {
            let now_instant = std::time::Instant::now();
            match cached.get() {
                Some((cached_utc, cached_instant))
                    if now_instant.duration_since(cached_instant)
                        < std::time::Duration::from_millis(50) =>
                {
                    cached_utc
                }
                _ => {
                    let now_utc = Utc::now();
                    cached.set(Some((now_utc, now_instant)));
                    now_utc
                }
            }
        })
    }

    pub fn is_expired(&self, exp: &Option<DateTime<Utc>>) -> bool {
        exp.map_or(false, |date| self.get_cached_now() > date)
    }

    /// Atomically check if entry is expired and remove it if so
    fn check_and_remove_if_expired(&self, key: &str) -> bool {
        if let Some(entry) = self.data.get(key) {
            let is_expired = match entry.value() {
                StoreValue::Simple(_, exp) => self.is_expired(exp),
                StoreValue::Hash(_, exp) => self.is_expired(exp),
                StoreValue::AtomicI64(_, exp) => self.is_expired(exp),
            };

            if is_expired {
                drop(entry); // Release read lock before removing
                self.data.remove(key);
                self.stats
                    .expired_entries_cleaned
                    .fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Atomically get a value if it exists and is not expired
    fn get_if_not_expired<T: DeserializeValue>(&self, key: &str) -> StoreResult<Option<T>> {
        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Simple(value, exp) => {
                    // Atomic check and access
                    if self.is_expired(exp) {
                        drop(entry); // Release read lock
                        self.data.remove(key);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::Relaxed);
                        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                        return Ok(None);
                    }

                    self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                    let deserialized = self.deserialize(value)?;
                    Ok(Some(deserialized))
                }
                StoreValue::Hash(_, _) => {
                    self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                    Ok(None) // Cannot get hash as simple value
                }
                StoreValue::AtomicI64(_, _) => {
                    self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                    Ok(None) // Cannot get AtomicI64 as simple value
                }
            }
        } else {
            self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
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
            StoreValue::AtomicI64(_, _) => {
                std::mem::size_of::<Arc<AtomicI64>>() + std::mem::size_of::<Option<DateTime<Utc>>>()
            }
        }
    }

    pub fn get_storage_stats(&self) -> StorageStats {
        let mut total_keys = 0;
        let mut simple_keys = 0;
        let mut hash_keys = 0;
        let mut total_hash_fields = 0;
        let mut estimated_memory_bytes = 0;

        for entry in self.data.iter() {
            total_keys += 1;
            let value = entry.value();
            estimated_memory_bytes += self.estimate_value_size(value);

            match value {
                StoreValue::Simple(_, _) => {
                    simple_keys += 1;
                }
                StoreValue::Hash(hash_map, _) => {
                    hash_keys += 1;
                    total_hash_fields += hash_map.len();
                }
                StoreValue::AtomicI64(_, _) => {
                    // Count AtomicI64 as simple key for stats purposes
                    simple_keys += 1;
                }
            }
        }

        StorageStats {
            total_keys,
            simple_keys,
            hash_keys,
            total_hash_fields,
            estimated_memory_bytes,
            operations: self.stats.to_operation_stats(),
        }
    }

    pub fn reset_stats(&self) {
        // Use SeqCst for consistency when resetting stats
        self.stats.gets.store(0, Ordering::SeqCst);
        self.stats.sets.store(0, Ordering::SeqCst);
        self.stats.deletes.store(0, Ordering::SeqCst);
        self.stats.exists_checks.store(0, Ordering::SeqCst);
        self.stats.hash_gets.store(0, Ordering::SeqCst);
        self.stats.hash_sets.store(0, Ordering::SeqCst);
        self.stats.hash_deletes.store(0, Ordering::SeqCst);
        self.stats.hash_exists_checks.store(0, Ordering::SeqCst);
        self.stats.hash_getalls.store(0, Ordering::SeqCst);
        self.stats.hash_keys_calls.store(0, Ordering::SeqCst);
        self.stats.hash_vals_calls.store(0, Ordering::SeqCst);
        self.stats.hash_len_calls.store(0, Ordering::SeqCst);
        self.stats.cache_hits.store(0, Ordering::SeqCst);
        self.stats.cache_misses.store(0, Ordering::SeqCst);
        self.stats
            .expired_entries_cleaned
            .store(0, Ordering::SeqCst);
    }

    pub fn get_memory_usage_bytes(&self) -> usize {
        self.get_storage_stats().estimated_memory_bytes
    }

    pub fn get_total_keys(&self) -> usize {
        self.data.len()
    }

    pub fn get_cache_hit_ratio(&self) -> f64 {
        let hits = self.stats.cache_hits.load(Ordering::Acquire);
        let misses = self.stats.cache_misses.load(Ordering::Acquire);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    fn serialize<T: SerializeValue>(&self, value: &T) -> StoreResult<Vec<u8>> {
        bincode::serde::encode_to_vec(value, BINCODE_CONFIG).map_err(|e| {
            StoreError::SerializationFailed(format!("Failed to serialize value: {}", e))
        })
    }

    fn deserialize<T: DeserializeValue>(&self, data: &[u8]) -> StoreResult<T> {
        let result = bincode::serde::decode_from_slice(data, BINCODE_CONFIG).map_err(|e| {
            StoreError::DeserializationFailed(format!("Failed to deserialize value: {}", e))
        });
        let data: T = result?.0;
        Ok(data)
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn get<T: DeserializeValue>(&self, key: &str) -> StoreResult<Option<T>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.gets.fetch_add(1, Ordering::Relaxed);
        self.get_if_not_expired(key)
    }

    async fn set<T: SerializeValue>(
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

        let data = self.serialize(value)?;

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

        // Check if expired and remove atomically
        if self.check_and_remove_if_expired(key) {
            return Ok(false);
        }

        Ok(self.data.contains_key(key))
    }

    async fn hset<T: SerializeValue>(
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

        let serialized_value = self.serialize(value)?;
        let field_exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));

        // Use atomic entry operation to avoid race conditions
        let entry = self
            .data
            .entry(key.to_string())
            .or_insert_with(|| StoreValue::Hash(DashMap::new(), None));

        match entry.value() {
            StoreValue::Hash(hash_map, exp) => {
                if self.is_expired(exp) {
                    // If the hash itself is expired, remove it and create a new one
                    drop(entry);
                    self.data.remove(key);
                    let new_hash_map = DashMap::new();
                    new_hash_map.insert(field.to_string(), (serialized_value, field_exp));
                    self.data
                        .insert(key.to_string(), StoreValue::Hash(new_hash_map, None));
                    self.stats
                        .expired_entries_cleaned
                        .fetch_add(1, Ordering::Relaxed);
                    return Ok(true);
                }
                let is_new = !hash_map.contains_key(field);
                hash_map.insert(field.to_string(), (serialized_value, field_exp));
                Ok(is_new)
            }
            StoreValue::Simple(_, _) => Ok(false), // Cannot set hash field on simple value
            StoreValue::AtomicI64(_, _) => Ok(false), // Cannot set hash field on AtomicI64
        }
    }

    async fn hget<T: DeserializeValue>(&self, key: &str, field: &str) -> StoreResult<Option<T>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if field.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "Field cannot be empty".to_string(),
            ));
        }

        self.stats.hash_gets.fetch_add(1, Ordering::Relaxed);

        // Check if expired and remove atomically
        if self.check_and_remove_if_expired(key) {
            self.stats.cache_misses.fetch_add(1, Ordering::Release);
            return Ok(None);
        }

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        drop(entry);
                        self.data.remove(key);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::Relaxed);
                        self.stats.cache_misses.fetch_add(1, Ordering::Release);
                        return Ok(None);
                    }

                    if let Some(field_entry) = hash_map.get(field) {
                        let (value, field_exp) = field_entry.value();
                        if self.is_expired(field_exp) {
                            drop(field_entry);
                            hash_map.remove(field);
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::SeqCst);
                            self.stats.cache_misses.fetch_add(1, Ordering::Release);
                            return Ok(None);
                        }

                        self.stats.cache_hits.fetch_add(1, Ordering::Acquire);
                        let deserialized = self.deserialize(value)?;
                        Ok(Some(deserialized))
                    } else {
                        self.stats.cache_misses.fetch_add(1, Ordering::Release);
                        Ok(None)
                    }
                }
                _ => {
                    self.stats.cache_misses.fetch_add(1, Ordering::Release);
                    Ok(None) // Key exists but is not a hash
                }
            }
        } else {
            self.stats.cache_misses.fetch_add(1, Ordering::Release);
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

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        drop(entry);
                        self.data.remove(key);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::SeqCst);
                        return Ok(false);
                    }
                    Ok(hash_map.remove(field).is_some())
                }
                _ => Ok(false),
            }
        } else {
            Ok(false)
        }
    }

    async fn hgetall<T: DeserializeValue>(&self, key: &str) -> StoreResult<HashMap<String, T>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.hash_getalls.fetch_add(1, Ordering::Relaxed);

        // Check if expired and remove atomically
        if self.check_and_remove_if_expired(key) {
            return Ok(HashMap::new());
        }

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        drop(entry);
                        self.data.remove(key);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::Relaxed);
                        return Ok(HashMap::new());
                    }

                    // Pre-allocate HashMap with estimated capacity for better performance
                    let mut result = if self.config.enable_preallocation {
                        HashMap::with_capacity(hash_map.len())
                    } else {
                        HashMap::new()
                    };
                    let now = self.get_cached_now();

                    // Collect expired fields to remove them atomically
                    let mut expired_fields = Vec::new();

                    for entry in hash_map.iter() {
                        let (field, (value, field_exp)) = entry.pair();
                        if field_exp.map_or(false, |exp| exp <= now) {
                            expired_fields.push(field.clone());
                        } else {
                            let deserialized = self.deserialize(value)?;
                            result.insert(field.clone(), deserialized);
                        }
                    }

                    // Remove expired fields
                    for field in expired_fields {
                        hash_map.remove(&field);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::Relaxed);
                    }

                    Ok(result)
                }
                _ => Ok(HashMap::new()), // Cannot get all fields from simple value
            }
        } else {
            Ok(HashMap::new())
        }
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

        self.stats.hash_exists_checks.fetch_add(1, Ordering::SeqCst);

        // Check if expired and remove atomically
        if self.check_and_remove_if_expired(key) {
            return Ok(false);
        }

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        drop(entry);
                        self.data.remove(key);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::SeqCst);
                        return Ok(false);
                    }

                    if let Some(field_entry) = hash_map.get(field) {
                        let (_, field_exp) = field_entry.value();
                        if self.is_expired(field_exp) {
                            drop(field_entry);
                            hash_map.remove(field);
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::SeqCst);
                            Ok(false)
                        } else {
                            Ok(true)
                        }
                    } else {
                        Ok(false)
                    }
                }
                _ => Ok(false), // Key exists but is not a hash
            }
        } else {
            Ok(false)
        }
    }

    async fn hkeys(&self, key: &str) -> StoreResult<Vec<String>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.hash_keys_calls.fetch_add(1, Ordering::SeqCst);

        // Check if expired and remove atomically
        if self.check_and_remove_if_expired(key) {
            return Ok(Vec::new());
        }

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        drop(entry);
                        self.data.remove(key);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::SeqCst);
                        return Ok(Vec::new());
                    }

                    let now = Utc::now();
                    let mut keys = Vec::new();
                    let mut expired_fields = Vec::new();

                    for entry in hash_map.iter() {
                        let (field, (_, field_exp)) = entry.pair();
                        if field_exp.map_or(false, |exp| exp <= now) {
                            expired_fields.push(field.clone());
                        } else {
                            keys.push(field.clone());
                        }
                    }

                    // Remove expired fields
                    for field in expired_fields {
                        hash_map.remove(&field);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::SeqCst);
                    }

                    Ok(keys)
                }
                _ => Ok(Vec::new()), // Cannot get keys from simple value
            }
        } else {
            Ok(Vec::new())
        }
    }

    async fn hvals<T: DeserializeValue>(&self, key: &str) -> StoreResult<Vec<T>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.hash_vals_calls.fetch_add(1, Ordering::SeqCst);

        // Check if expired and remove atomically
        if self.check_and_remove_if_expired(key) {
            return Ok(Vec::new());
        }

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        drop(entry);
                        self.data.remove(key);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::SeqCst);
                        return Ok(Vec::new());
                    }

                    let now = Utc::now();
                    let mut values = Vec::new();
                    let mut expired_fields = Vec::new();

                    for entry in hash_map.iter() {
                        let (field, (value, field_exp)) = entry.pair();
                        if field_exp.map_or(false, |exp| exp <= now) {
                            expired_fields.push(field.clone());
                        } else {
                            let deserialized = self.deserialize(value)?;
                            values.push(deserialized);
                        }
                    }

                    // Remove expired fields
                    for field in expired_fields {
                        hash_map.remove(&field);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::SeqCst);
                    }

                    Ok(values)
                }
                _ => Ok(Vec::new()), // Cannot get values from simple value or AtomicI64
            }
        } else {
            Ok(Vec::new())
        }
    }

    async fn hlen(&self, key: &str) -> StoreResult<usize> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.hash_len_calls.fetch_add(1, Ordering::SeqCst);

        // Check if expired and remove atomically
        if self.check_and_remove_if_expired(key) {
            return Ok(0);
        }

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        drop(entry);
                        self.data.remove(key);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::SeqCst);
                        return Ok(0);
                    }

                    // Clean expired fields and count valid ones
                    let now = Utc::now();
                    let mut valid_count = 0;
                    let mut expired_fields = Vec::new();

                    for entry in hash_map.iter() {
                        let (field, (_, field_exp)) = entry.pair();
                        if field_exp.map_or(false, |exp| exp <= now) {
                            expired_fields.push(field.clone());
                        } else {
                            valid_count += 1;
                        }
                    }

                    // Remove expired fields
                    for field in expired_fields {
                        hash_map.remove(&field);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::SeqCst);
                    }

                    Ok(valid_count)
                }
                _ => Ok(0), // Cannot get length from simple value or AtomicI64
            }
        } else {
            Ok(0)
        }
    }

    /// Compare and swap operation for atomic updates
    async fn compare_and_swap<T: SerializeValue + DeserializeValue>(
        &self,
        key: &str,
        expected: &T,
        new_value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        let expected_str = self.serialize(expected)?;
        let new_str = self.serialize(new_value)?;

        let exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));

        if let Some(mut entry) = self.data.get_mut(key) {
            match entry.value_mut() {
                StoreValue::Simple(current, current_exp) => {
                    if !self.is_expired(current_exp) && current == &expected_str {
                        *current = new_str;
                        *current_exp = exp;
                        self.stats.sets.fetch_add(1, Ordering::SeqCst);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                _ => Ok(false),
            }
        } else {
            Ok(false)
        }
    }
}

#[async_trait]
impl AtomicStore for MemoryStore {
    async fn get_i64(&self, key: &str) -> StoreResult<Option<i64>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.gets.fetch_add(1, Ordering::SeqCst);

        // Check if expired and remove atomically
        if self.check_and_remove_if_expired(key) {
            self.stats.cache_misses.fetch_add(1, Ordering::Release);
            return Ok(None);
        }

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::AtomicI64(atomic, exp) => {
                    if self.is_expired(exp) {
                        drop(entry);
                        self.data.remove(key);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::SeqCst);
                        self.stats.cache_misses.fetch_add(1, Ordering::Release);
                        return Ok(None);
                    }

                    self.stats.cache_hits.fetch_add(1, Ordering::Acquire);
                    Ok(Some(atomic.load(Ordering::SeqCst)))
                }
                _ => {
                    self.stats.cache_misses.fetch_add(1, Ordering::Release);
                    Ok(None) // Key exists but is not an AtomicI64
                }
            }
        } else {
            self.stats.cache_misses.fetch_add(1, Ordering::Release);
            Ok(None)
        }
    }

    async fn set_i64(&self, key: &str, value: i64, ttl: Option<u64>) -> StoreResult<()> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        self.stats.sets.fetch_add(1, Ordering::SeqCst);

        let atomic = Arc::new(AtomicI64::new(value));
        let exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));
        self.data
            .insert(key.to_string(), StoreValue::AtomicI64(atomic, exp));
        Ok(())
    }

    async fn incr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));

        let mut entry = self
            .data
            .entry(key.to_string())
            .or_insert_with(|| StoreValue::AtomicI64(Arc::new(AtomicI64::new(0)), None));

        match entry.value_mut() {
            StoreValue::AtomicI64(atomic, current_exp) => {
                if self.is_expired(current_exp) {
                    // If expired, reset to 'by' value
                    let new_atomic = Arc::new(AtomicI64::new(by));
                    *entry = StoreValue::AtomicI64(new_atomic.clone(), exp);
                    self.stats
                        .expired_entries_cleaned
                        .fetch_add(1, Ordering::SeqCst);
                    self.stats.sets.fetch_add(1, Ordering::SeqCst);
                    return Ok(Some(by));
                }

                let new_value = atomic.fetch_add(by, Ordering::SeqCst) + by;
                // Update expiration if TTL is provided
                if ttl.is_some() {
                    *current_exp = exp;
                }
                self.stats.sets.fetch_add(1, Ordering::SeqCst);
                Ok(Some(new_value))
            }
            _ => Err(StoreError::TypeMismatch(
                "Key exists but is not an AtomicI64".to_string(),
            )),
        }
    }

    async fn decr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));

        let mut entry = self
            .data
            .entry(key.to_string())
            .or_insert_with(|| StoreValue::AtomicI64(Arc::new(AtomicI64::new(0)), None));

        match entry.value_mut() {
            StoreValue::AtomicI64(atomic, current_exp) => {
                if self.is_expired(current_exp) {
                    // If expired, reset to -by value
                    let new_atomic = Arc::new(AtomicI64::new(-by));
                    *entry = StoreValue::AtomicI64(new_atomic.clone(), exp);
                    self.stats
                        .expired_entries_cleaned
                        .fetch_add(1, Ordering::SeqCst);
                    self.stats.sets.fetch_add(1, Ordering::SeqCst);
                    return Ok(Some(-by));
                }

                let new_value = atomic.fetch_sub(by, Ordering::SeqCst) - by;
                // Update expiration if TTL is provided
                if ttl.is_some() {
                    *current_exp = exp;
                }
                self.stats.sets.fetch_add(1, Ordering::SeqCst);
                Ok(Some(new_value))
            }
            _ => Err(StoreError::TypeMismatch(
                "Key exists but is not an AtomicI64".to_string(),
            )),
        }
    }

    async fn compare_and_swap_i64(
        &self,
        key: &str,
        old: i64,
        new: i64,
        ttl: Option<u64>,
    ) -> StoreResult<bool> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));

        if let Some(mut entry) = self.data.get_mut(key) {
            match entry.value_mut() {
                StoreValue::AtomicI64(atomic, current_exp) => {
                    if self.is_expired(current_exp) {
                        // If expired, reset to new value
                        let new_atomic = Arc::new(AtomicI64::new(new));
                        *entry = StoreValue::AtomicI64(new_atomic.clone(), exp);
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::SeqCst);
                        return Ok(true);
                    }

                    let current_value = atomic.load(Ordering::SeqCst);
                    if current_value == old {
                        atomic.store(new, Ordering::SeqCst);
                        // Update expiration if TTL is provided
                        if ttl.is_some() {
                            *current_exp = exp;
                        }
                        self.stats.sets.fetch_add(1, Ordering::SeqCst);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                _ => Err(StoreError::TypeMismatch(
                    "Key exists but is not an AtomicI64".to_string(),
                )),
            }
        } else {
            Ok(false)
        }
    }
}

impl MemoryStore {
    pub async fn measure_and_set_i64<F>(
        &self,
        key: &str,
        measure_fn: F,
        ttl: Option<u64>,
    ) -> StoreResult<(bool, i64)>
    where
        F: Fn(i64) -> (bool, i64) + Send + Sync,
    {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));

        let mut entry = self
            .data
            .entry(key.to_string())
            .or_insert_with(|| StoreValue::AtomicI64(Arc::new(AtomicI64::new(0)), None));

        match entry.value_mut() {
            StoreValue::AtomicI64(atomic, current_exp) => {
                if self.is_expired(current_exp) {
                    // If expired, reset to measure_fn(0)
                    let (res, new_value) = measure_fn(0);
                    let new_atomic = Arc::new(AtomicI64::new(new_value));
                    *entry = StoreValue::AtomicI64(new_atomic.clone(), exp);
                    self.stats
                        .expired_entries_cleaned
                        .fetch_add(1, Ordering::SeqCst);
                    self.stats.sets.fetch_add(1, Ordering::SeqCst);
                    return Ok((res, new_value));
                }

                let current_value = atomic.load(Ordering::SeqCst);
                let (res, new_value) = measure_fn(current_value);
                atomic.store(new_value, Ordering::SeqCst);
                // Update expiration if TTL is provided
                if ttl.is_some() {
                    *current_exp = exp;
                }
                self.stats.sets.fetch_add(1, Ordering::SeqCst);
                Ok((res, new_value))
            }
            _ => Err(StoreError::TypeMismatch(
                "Key exists but is not an AtomicI64".to_string(),
            )),
        }
    }
}

impl MemoryStore {
    /// Batch get operations for better performance - MemoryStore specific
    pub async fn batch_get<T: DeserializeValue>(
        &self,
        keys: &[&str],
    ) -> StoreResult<Vec<Option<T>>> {
        let mut results = Vec::with_capacity(keys.len());

        for &key in keys {
            if key.trim().is_empty() {
                results.push(None);
                continue;
            }

            self.stats.gets.fetch_add(1, Ordering::Relaxed);

            match self.get_if_not_expired(key) {
                Ok(value) => results.push(value),
                Err(_) => results.push(None), // Convert errors to None for batch operations
            }
        }

        Ok(results)
    }

    /// Batch set operations for better performance - MemoryStore specific
    pub async fn batch_set<T: SerializeValue>(
        &self,
        operations: &[(&str, &T, Option<u64>)],
    ) -> StoreResult<Vec<bool>> {
        let mut results = Vec::with_capacity(operations.len());

        for &(key, value, ttl) in operations {
            if key.trim().is_empty() {
                results.push(false);
                continue;
            }

            if let Some(ttl_val) = ttl {
                if ttl_val == 0 {
                    results.push(false);
                    continue;
                }
            }

            self.stats.sets.fetch_add(1, Ordering::Relaxed);

            match self.serialize(value) {
                Ok(data) => {
                    let exp =
                        ttl.map(|t| self.get_cached_now() + chrono::Duration::seconds(t as i64));
                    self.data
                        .insert(key.to_string(), StoreValue::Simple(data, exp));
                    results.push(true);
                }
                Err(_) => results.push(false),
            }
        }

        Ok(results)
    }

    /// Batch delete operations for better performance - MemoryStore specific
    pub async fn batch_delete(&self, keys: &[&str]) -> StoreResult<Vec<bool>> {
        let mut results = Vec::with_capacity(keys.len());

        for &key in keys {
            if key.trim().is_empty() {
                results.push(false);
                continue;
            }

            self.stats.deletes.fetch_add(1, Ordering::Relaxed);
            results.push(self.data.remove(key).is_some());
        }

        Ok(results)
    }

    /// Batch exists operations for better performance - MemoryStore specific
    pub async fn batch_exists(&self, keys: &[&str]) -> StoreResult<Vec<bool>> {
        let mut results = Vec::with_capacity(keys.len());

        for &key in keys {
            if key.trim().is_empty() {
                results.push(false);
                continue;
            }

            self.stats.exists_checks.fetch_add(1, Ordering::Relaxed);

            // Check if expired and remove atomically
            if self.check_and_remove_if_expired(key) {
                results.push(false);
            } else {
                results.push(self.data.contains_key(key));
            }
        }

        Ok(results)
    }

    /// Batch cleanup of expired entries - MemoryStore specific
    /// Uses config.cleanup_batch_size if no max_items specified
    pub async fn batch_cleanup_expired(&self, max_items: Option<usize>) -> usize {
        let effective_batch_size = max_items.unwrap_or(self.config.cleanup_batch_size);
        let mut removed: usize = 0;
        let mut checked = 0;
        let now = self.get_cached_now();

        // Apply memory pressure adjustment to batch size
        let adjusted_batch_size = if self.is_memory_pressure() {
            effective_batch_size * 2 // Clean more aggressively under memory pressure
        } else {
            effective_batch_size
        };

        self.data.retain(|_, value| {
            if checked >= adjusted_batch_size {
                return true; // Keep remaining items for next batch
            }
            checked += 1;

            let should_keep = match value {
                StoreValue::Simple(_, exp) => !self.is_expired(exp),
                StoreValue::Hash(hash_map, exp) => {
                    // Clean expired hash fields
                    let mut fields_removed = 0;
                    hash_map.retain(|_, (_, field_exp)| {
                        let keep = field_exp.map_or(true, |e| e > now);
                        if !keep {
                            fields_removed += 1;
                        }
                        keep
                    });
                    removed += fields_removed;

                    // Keep hash if not expired and has fields
                    !self.is_expired(exp) && !hash_map.is_empty()
                }
                StoreValue::AtomicI64(_, exp) => !self.is_expired(exp),
            };

            if !should_keep {
                removed += 1;
            }
            should_keep
        });

        self.stats
            .expired_entries_cleaned
            .fetch_add(removed as u64, Ordering::Relaxed);
        removed
    }

    /// Cleanup expired entries using default config settings
    pub async fn cleanup_expired(&self) -> usize {
        self.batch_cleanup_expired(None).await
    }

    /// Get detailed performance metrics - MemoryStore specific
    pub fn get_performance_metrics(&self) -> MemoryStorePerformanceMetrics {
        let stats = self.stats.to_operation_stats();
        let total_operations = stats.gets + stats.sets + stats.deletes + stats.exists_checks;
        let total_hash_operations = stats.hash_gets
            + stats.hash_sets
            + stats.hash_deletes
            + stats.hash_exists_checks
            + stats.hash_getalls
            + stats.hash_keys_calls
            + stats.hash_vals_calls
            + stats.hash_len_calls;

        MemoryStorePerformanceMetrics {
            cache_hit_ratio: self.get_cache_hit_ratio(),
            total_operations,
            total_hash_operations,
            expired_cleanup_efficiency: if total_operations > 0 {
                stats.expired_entries_cleaned as f64 / total_operations as f64
            } else {
                0.0
            },
            memory_usage_bytes: self.get_memory_usage_bytes(),
            total_keys: self.get_total_keys(),
            average_key_size: if self.get_total_keys() > 0 {
                self.get_memory_usage_bytes() / self.get_total_keys()
            } else {
                0
            },
        }
    }

    /// Get current memory efficiency statistics - MemoryStore specific
    pub fn get_memory_efficiency(&self) -> MemoryEfficiencyStats {
        let total_keys = self.data.len();
        let mut simple_keys = 0;
        let mut hash_keys = 0;
        let mut atomic_keys = 0;
        let mut total_hash_fields = 0;
        let mut expired_keys = 0;

        let now = self.get_cached_now();

        for entry in self.data.iter() {
            match entry.value() {
                StoreValue::Simple(_, exp) => {
                    simple_keys += 1;
                    if self.is_expired(exp) {
                        expired_keys += 1;
                    }
                }
                StoreValue::Hash(hash_map, exp) => {
                    hash_keys += 1;
                    total_hash_fields += hash_map.len();
                    if self.is_expired(exp) {
                        expired_keys += 1;
                    } else {
                        // Count expired fields within the hash
                        for field_entry in hash_map.iter() {
                            let (_, (_, field_exp)) = field_entry.pair();
                            if field_exp.map_or(false, |exp| exp <= now) {
                                expired_keys += 1;
                            }
                        }
                    }
                }
                StoreValue::AtomicI64(_, exp) => {
                    atomic_keys += 1;
                    if self.is_expired(exp) {
                        expired_keys += 1;
                    }
                }
            }
        }

        MemoryEfficiencyStats {
            total_keys,
            simple_keys,
            hash_keys,
            atomic_keys,
            total_hash_fields,
            expired_keys,
            fragmentation_ratio: if total_keys > 0 {
                expired_keys as f64 / total_keys as f64
            } else {
                0.0
            },
        }
    }

    /// Check if memory usage exceeds configured limits - MemoryStore specific
    pub fn is_memory_pressure(&self) -> bool {
        if self.config.max_memory_usage == 0 {
            return false; // Memory limiting disabled
        }

        self.get_memory_usage_bytes() > self.config.max_memory_usage
    }

    /// Get recommendations for performance optimization - MemoryStore specific
    pub fn get_optimization_recommendations(&self) -> Vec<String> {
        let mut recommendations = Vec::new();
        let efficiency = self.get_memory_efficiency();
        let perf_metrics = self.get_performance_metrics();

        // Check fragmentation
        if efficiency.fragmentation_ratio > 0.1 {
            recommendations.push(format!(
                "High fragmentation detected ({:.1}%). Consider running cleanup more frequently.",
                efficiency.fragmentation_ratio * 100.0
            ));
        }

        // Check cache hit ratio
        if perf_metrics.cache_hit_ratio < 0.8 {
            recommendations.push(format!(
                "Low cache hit ratio ({:.1}%). Consider increasing cache size or reviewing access patterns.",
                perf_metrics.cache_hit_ratio * 100.0
            ));
        }

        // Check memory pressure
        if self.is_memory_pressure() {
            recommendations.push(
                "Memory usage exceeds configured limit. Consider reducing TTL or increasing cleanup frequency.".to_string()
            );
        }

        // Check operation distribution
        if perf_metrics.total_hash_operations > perf_metrics.total_operations * 2 {
            recommendations.push(
                "Heavy hash usage detected. Consider enabling hash-specific optimizations."
                    .to_string(),
            );
        }

        if recommendations.is_empty() {
            recommendations.push("Performance looks optimal!".to_string());
        }

        recommendations
    }

    /// Adaptive cleaner that uses configuration parameters effectively
    pub fn run_adaptive_cleaner(&self, base_interval: u64) {
        let data = self.data.clone();
        let stats = self.stats.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            log::info!(
                "MemoryStore adaptive cleaner started with base interval: {} seconds, batch size: {}",
                base_interval,
                config.cleanup_batch_size
            );

            let mut current_interval = base_interval;
            let mut ticker =
                tokio::time::interval(tokio::time::Duration::from_secs(current_interval));

            loop {
                ticker.tick().await;

                let start_time = std::time::Instant::now();
                let data_size_before = data.len();
                let now = if config.enable_time_caching {
                    // Use a single time for the entire cleanup cycle
                    Utc::now()
                } else {
                    Utc::now()
                };

                let mut removed: u64 = 0;
                let mut processed: usize = 0;

                // Adaptive batch processing
                let effective_batch_size = if config.max_memory_usage > 0 {
                    // Increase batch size if we're approaching memory limits
                    let estimated_memory = data.len() * 100; // rough estimate
                    if estimated_memory > config.max_memory_usage * 80 / 100 {
                        config.cleanup_batch_size * 2
                    } else {
                        config.cleanup_batch_size
                    }
                } else {
                    config.cleanup_batch_size
                };

                // Use retain with configurable batch processing
                data.retain(|_, value| {
                    if processed >= effective_batch_size {
                        return true; // Keep remaining for next batch
                    }
                    processed += 1;

                    match value {
                        StoreValue::Hash(hash_map, exp) => {
                            // Clean expired fields within the hash atomically
                            let mut fields_removed = 0;
                            hash_map.retain(|_, (_, field_exp)| {
                                let keep = field_exp.map_or(true, |exp_time| exp_time > now);
                                if !keep {
                                    fields_removed += 1;
                                }
                                keep
                            });
                            removed += fields_removed;

                            // Keep the hash if it's not expired and has fields
                            let keep_hash =
                                exp.map_or(true, |exp_time| exp_time > now) && !hash_map.is_empty();
                            if !keep_hash {
                                removed += 1;
                            }
                            keep_hash
                        }
                        StoreValue::Simple(_, exp) => {
                            let keep = exp.map_or(true, |exp_time| exp_time > now);
                            if !keep {
                                removed += 1;
                            }
                            keep
                        }
                        StoreValue::AtomicI64(_, exp) => {
                            let keep = exp.map_or(true, |exp_time| exp_time > now);
                            if !keep {
                                removed += 1;
                            }
                            keep
                        }
                    }
                });

                let cleanup_duration = start_time.elapsed();
                let data_size_after = data.len();

                // Adaptive interval adjustment based on effectiveness
                if removed > (effective_batch_size / 2) as u64 {
                    // High expiration rate, clean more frequently
                    current_interval = std::cmp::max(base_interval / 2, 1);
                } else if removed < (effective_batch_size / 10) as u64 {
                    // Low expiration rate, clean less frequently
                    current_interval = std::cmp::min(base_interval * 2, 300);
                } else {
                    current_interval = base_interval;
                }

                // Update ticker with new interval
                ticker = tokio::time::interval(tokio::time::Duration::from_secs(current_interval));

                if removed > 0 {
                    stats
                        .expired_entries_cleaned
                        .fetch_add(removed as u64, Ordering::Relaxed);
                    log::info!(
                        "MemoryStore cleaner: removed {} expired entries, processed {}/{} items in {:?}, next interval: {}s",
                        removed,
                        processed,
                        data_size_before,
                        cleanup_duration,
                        current_interval
                    );
                }

                // Memory pressure check
                if config.max_memory_usage > 0 && data_size_after > config.max_memory_usage / 100 {
                    log::warn!(
                        "MemoryStore approaching memory limit: {} keys (estimated {} bytes)",
                        data_size_after,
                        data_size_after * 100
                    );
                }
            }
        });
    }

    /// Legacy cleaner method - maintained for backward compatibility
    pub fn run_cleaner(&self, interval: u64) {
        self.run_adaptive_cleaner(interval);
    }
}
