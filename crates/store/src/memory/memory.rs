use super::capacity::Capacity;
use super::config::MemoryStoreConfig;
use super::stats::{
    AtomicOperationStats, MemoryEfficiencyStats, MemoryStorePerformanceMetrics, StorageStats,
};
use crate::error::{StoreError, StoreResult};
use crate::store::{AtomicStore, DeserializeValue, SerializeValue, Store};
use crate::transaction::{Requirement, Transaction, Write};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use dashmap::mapref::entry::Entry as DashEntry;
use portable_atomic::AtomicI64;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};

pub type Data = Arc<[u8]>;
pub type HashFields = DashMap<String, (Data, Option<DateTime<Utc>>)>;

#[derive(Debug, Clone)]
pub enum StoreValue {
    Simple(Data, Option<DateTime<Utc>>),
    Hash(HashFields, Option<DateTime<Utc>>),
    AtomicI64(Arc<AtomicI64>, Option<DateTime<Utc>>),
}

/// Shards of a hash value. A write to a hash already holds its key's slot in
/// the outer map, so more shards would only add memory: each is a lock padded
/// to a cache line.
const HASH_SHARDS: usize = 2;
const HASH_SHARD_BYTES: usize = 128;
/// Reference counts in front of an `Arc` allocation.
const ARC_HEADER_BYTES: usize = 2 * std::mem::size_of::<usize>();

/// An empty hash value.
pub(crate) fn new_hash() -> HashFields {
    DashMap::with_shard_amount(HASH_SHARDS)
}

/// Estimated bytes one entry holds: its slot in the map, the key and the
/// value's allocations. Allocator and table overhead are left out; the
/// estimate bounds memory, it does not account for it exactly.
fn entry_size(key: &str, value: &StoreValue) -> usize {
    slot_size(key)
        + match value {
            StoreValue::Simple(data, _) => ARC_HEADER_BYTES + data.len(),
            StoreValue::AtomicI64(..) => ARC_HEADER_BYTES + std::mem::size_of::<AtomicI64>(),
            StoreValue::Hash(fields, _) => {
                HASH_SHARDS * HASH_SHARD_BYTES
                    + fields
                        .iter()
                        .map(|field| field_size(field.key(), &field.value().0))
                        .sum::<usize>()
            }
        }
}

/// Estimated bytes of a key's slot in the map, before its value.
fn slot_size(key: &str) -> usize {
    std::mem::size_of::<(String, StoreValue)>() + key.len()
}

/// Estimated bytes one hash field adds.
fn field_size(field: &str, data: &Data) -> usize {
    std::mem::size_of::<(String, (Data, Option<DateTime<Utc>>))>()
        + field.len()
        + ARC_HEADER_BYTES
        + data.len()
}

pub struct MemoryStore {
    pub(crate) data: Arc<DashMap<String, StoreValue>>,
    pub(crate) stats: Arc<AtomicOperationStats>,
    config: MemoryStoreConfig,
    capacity: Arc<Capacity>,
    /// Held while a transaction checks and writes its keys.
    transactions: Arc<Mutex<()>>,
}

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        MemoryStore {
            data: Arc::clone(&self.data),
            stats: Arc::clone(&self.stats),
            config: self.config.clone(),
            capacity: Arc::clone(&self.capacity),
            transactions: Arc::clone(&self.transactions),
        }
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::with_config(MemoryStoreConfig::default())
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStore {
    pub fn with_config(config: MemoryStoreConfig) -> Self {
        let data = if config.initial_capacity > 0 {
            Arc::new(DashMap::with_capacity(config.initial_capacity))
        } else {
            Arc::new(DashMap::new())
        };

        Self {
            data,
            stats: Arc::new(AtomicOperationStats::default()),
            capacity: Arc::new(Capacity::new(config.max_memory_usage)),
            config,
            transactions: Arc::default(),
        }
    }

    pub fn get_config(&self) -> &MemoryStoreConfig {
        &self.config
    }

    /// Replace this store's contents with the contents of `other` and reset
    /// operational stats. Intended for restoring a snapshot into an already
    /// initialized (e.g. global) store.
    ///
    /// A restored snapshot is never truncated: if it exceeds the memory limit,
    /// the store creates nothing until expiry frees room.
    pub fn absorb(&self, other: &MemoryStore) {
        self.data.clear();
        for entry in other.data.iter() {
            self.data.insert(entry.key().clone(), entry.value().clone());
        }
        self.reset_stats();
        self.sweep();
    }

    /// Get cached current time to reduce syscalls
    fn get_cached_now(&self) -> DateTime<Utc> {
        if !self.config.enable_time_caching {
            return Utc::now();
        }

        thread_local! {
            static CACHED_TIME: std::cell::Cell<Option<(DateTime<Utc>, std::time::Instant)>> = const { std::cell::Cell::new(None) };
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

    /// Compute an absolute expiration from a TTL in seconds.
    ///
    /// Out-of-range TTLs saturate to the far future instead of wrapping
    /// negative or panicking inside chrono arithmetic.
    #[inline]
    fn expiration_from_ttl(&self, ttl: Option<u64>) -> Option<DateTime<Utc>> {
        ttl.map(|seconds| {
            i64::try_from(seconds)
                .ok()
                .and_then(chrono::Duration::try_seconds)
                .and_then(|duration| self.get_cached_now().checked_add_signed(duration))
                .unwrap_or(DateTime::<Utc>::MAX_UTC)
        })
    }

    #[inline]
    fn wrong_type_error(&self) -> StoreError {
        StoreError::TypeMismatch(
            "WRONGTYPE Operation against a key holding the wrong kind of value".to_string(),
        )
    }

    /// Writes `value` under `key`, replacing any value; a new key needs room.
    fn insert(&self, key: &str, value: StoreValue) -> StoreResult<()> {
        match self.data.entry(key.to_string()) {
            DashEntry::Vacant(entry) => {
                self.capacity.admit(key, entry_size(key, &value))?;
                entry.insert(value);
            }
            DashEntry::Occupied(mut entry) => {
                entry.insert(value);
            }
        }
        Ok(())
    }

    /// The entry of a counter, created at zero when absent and room allows.
    fn counter(
        &self,
        key: &str,
    ) -> StoreResult<dashmap::mapref::one::RefMut<'_, String, StoreValue>> {
        match self.data.entry(key.to_string()) {
            DashEntry::Occupied(entry) => Ok(entry.into_ref()),
            DashEntry::Vacant(entry) => {
                let value = StoreValue::AtomicI64(Arc::new(AtomicI64::new(0)), None);
                self.capacity.admit(key, entry_size(key, &value))?;
                Ok(entry.insert(value))
            }
        }
    }

    #[inline]
    fn is_expired_at(&self, exp: &Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
        exp.is_some_and(|date| now > date)
    }

    #[inline]
    fn store_value_is_expired_at(&self, value: &StoreValue, now: DateTime<Utc>) -> bool {
        match value {
            StoreValue::Simple(_, exp) => self.is_expired_at(exp, now),
            StoreValue::Hash(_, exp) => self.is_expired_at(exp, now),
            StoreValue::AtomicI64(_, exp) => self.is_expired_at(exp, now),
        }
    }

    #[inline]
    fn remove_key_if_expired_at(&self, key: &str, now: DateTime<Utc>) -> bool {
        self.data
            .remove_if(key, |_, value| self.store_value_is_expired_at(value, now))
            .is_some()
    }

    pub fn is_expired(&self, exp: &Option<DateTime<Utc>>) -> bool {
        self.is_expired_at(exp, self.get_cached_now())
    }

    /// Remove `key` if still expired at `now`, updating cleanup stats.
    /// The caller must not hold a guard for `key` (would deadlock the shard).
    fn remove_expired_key(&self, key: &str, now: DateTime<Utc>) {
        if self.remove_key_if_expired_at(key, now) {
            self.stats
                .expired_entries_cleaned
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Check whether a key exists and is not expired, removing it when expired.
    /// Only takes a shard write lock when an expired entry actually needs removal.
    fn key_exists_unexpired(&self, key: &str) -> bool {
        if let Some(entry) = self.data.get(key) {
            let now = self.get_cached_now();
            if self.store_value_is_expired_at(entry.value(), now) {
                drop(entry);
                self.remove_expired_key(key, now);
                return false;
            }
            true
        } else {
            false
        }
    }

    /// Atomically get a value if it exists and is not expired
    fn get_if_not_expired<T: DeserializeValue>(&self, key: &str) -> StoreResult<Option<T>> {
        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Simple(value, exp) => {
                    // Atomic check and access
                    let now = self.get_cached_now();
                    if self.is_expired_at(exp, now) {
                        drop(entry); // Release read lock
                        if self.remove_key_if_expired_at(key, now) {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                        return Ok(None);
                    }

                    self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                    let deserialized = self.deserialize(value)?;
                    Ok(Some(deserialized))
                }
                other => {
                    // Expired key of another type behaves as missing (Redis
                    // semantics); only a live value is a type error.
                    let now = self.get_cached_now();
                    if self.store_value_is_expired_at(other, now) {
                        drop(entry);
                        self.remove_expired_key(key, now);
                        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                        return Ok(None);
                    }
                    self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                    Err(self.wrong_type_error())
                }
            }
        } else {
            self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
            Ok(None)
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
            estimated_memory_bytes += entry_size(entry.key(), value);

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

    /// When `key` expires, for diagnostics and tests. `None` for a missing key
    /// or one stored without a deadline.
    pub fn expiry(&self, key: &str) -> Option<DateTime<Utc>> {
        let now = self.get_cached_now();
        let expiration = match self.data.get(key)?.value() {
            StoreValue::Simple(_, expiration)
            | StoreValue::Hash(_, expiration)
            | StoreValue::AtomicI64(_, expiration) => *expiration,
        };
        expiration.filter(|expiration| *expiration > now)
    }

    pub fn get_cache_hit_ratio(&self) -> f64 {
        let hits = self.stats.cache_hits.load(Ordering::Relaxed);
        let misses = self.stats.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Returns a monotonic approximation of persistence-relevant mutations.
    /// This is intentionally conservative: some no-op writes may still advance it.
    pub fn persistence_revision(&self) -> u64 {
        self.stats.sets.load(Ordering::Relaxed)
            + self.stats.deletes.load(Ordering::Relaxed)
            + self.stats.hash_sets.load(Ordering::Relaxed)
            + self.stats.hash_deletes.load(Ordering::Relaxed)
            + self.stats.expired_entries_cleaned.load(Ordering::Relaxed)
    }

    fn serialize<T: SerializeValue>(&self, value: &T) -> StoreResult<Vec<u8>> {
        rmp_serde::to_vec(value).map_err(|e| {
            StoreError::SerializationFailed(format!("Failed to serialize value: {}", e))
        })
    }

    fn deserialize<T: DeserializeValue>(&self, data: &[u8]) -> StoreResult<T> {
        rmp_serde::from_slice(data).map_err(|e| {
            StoreError::DeserializationFailed(format!("Failed to deserialize value: {}", e))
        })
    }

    fn holds(&self, requirement: &Requirement, now: DateTime<Utc>) -> StoreResult<bool> {
        let Some(entry) = self.data.get(requirement.key()) else {
            return Ok(false);
        };
        if self.store_value_is_expired_at(entry.value(), now) {
            return Ok(false);
        }
        match (requirement, entry.value()) {
            (Requirement::Exists(_), _) => Ok(true),
            (Requirement::Equals(_, expected), StoreValue::Simple(current, _)) => {
                Ok(current.as_ref() == expected.as_slice())
            }
            (Requirement::Equals(..), _) => Err(self.wrong_type_error()),
        }
    }

    fn apply(&self, write: &Write, now: DateTime<Utc>) {
        match write {
            Write::Set(key, value, ttl) => {
                let expiration = self.expiration_from_ttl(Some(*ttl));
                self.data.insert(
                    key.clone(),
                    StoreValue::Simple(value.as_slice().into(), expiration),
                );
                self.stats.sets.fetch_add(1, Ordering::Relaxed);
            }
            Write::Replace(key, value) => {
                if let Some(mut entry) = self.data.get_mut(key)
                    && let StoreValue::Simple(current, _) = entry.value_mut()
                {
                    *current = value.as_slice().into();
                    self.stats.sets.fetch_add(1, Ordering::Relaxed);
                }
            }
            Write::Expire(key, ttl) => {
                let Some(mut entry) = self.data.get_mut(key) else {
                    return;
                };
                let (StoreValue::Simple(_, expiration)
                | StoreValue::Hash(_, expiration)
                | StoreValue::AtomicI64(_, expiration)) = entry.value_mut();
                if !self.is_expired_at(expiration, now) {
                    *expiration = self.expiration_from_ttl(Some(*ttl));
                    self.stats.sets.fetch_add(1, Ordering::Relaxed);
                }
            }
            Write::Delete(key) => {
                self.data.remove(key);
                self.stats.deletes.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn ping(&self) -> StoreResult<()> {
        Ok(())
    }

    async fn get<T: DeserializeValue>(&self, key: &str) -> StoreResult<Option<T>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.gets.fetch_add(1, Ordering::Relaxed);
        self.get_if_not_expired(key)
    }

    async fn take<T: DeserializeValue>(&self, key: &str) -> StoreResult<Option<T>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        let DashEntry::Occupied(entry) = self.data.entry(key.to_string()) else {
            return Ok(None);
        };
        if self.store_value_is_expired_at(entry.get(), self.get_cached_now()) {
            entry.remove();
            self.stats
                .expired_entries_cleaned
                .fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        }
        if !matches!(entry.get(), StoreValue::Simple(..)) {
            return Err(self.wrong_type_error());
        }

        let StoreValue::Simple(data, _) = entry.remove() else {
            unreachable!("entry type checked under the same lock");
        };
        self.stats.deletes.fetch_add(1, Ordering::Relaxed);
        self.deserialize(&data).map(Some)
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

        if ttl == Some(0) {
            return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
        }

        let data: Data = self.serialize(value)?.into();
        let exp = self.expiration_from_ttl(ttl);
        self.insert(key, StoreValue::Simple(data, exp))?;
        self.stats.sets.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn set_if_absent<T: SerializeValue>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if ttl == Some(0) {
            return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
        }

        let data: Data = self.serialize(value)?.into();
        let exp = self.expiration_from_ttl(ttl);
        let new_value = StoreValue::Simple(data, exp);

        match self.data.entry(key.to_string()) {
            DashEntry::Vacant(entry) => {
                self.capacity.admit(key, entry_size(key, &new_value))?;
                entry.insert(new_value);
            }
            DashEntry::Occupied(mut entry) => {
                if !self.store_value_is_expired_at(entry.get(), self.get_cached_now()) {
                    return Ok(false);
                }
                entry.insert(new_value);
                self.stats
                    .expired_entries_cleaned
                    .fetch_add(1, Ordering::Relaxed);
            }
        }

        self.stats.sets.fetch_add(1, Ordering::Relaxed);
        Ok(true)
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

        Ok(self.key_exists_unexpired(key))
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
        if ttl == Some(0) {
            return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
        }

        self.stats.hash_sets.fetch_add(1, Ordering::Relaxed);

        let serialized_value: Data = self.serialize(value)?.into();
        let field_exp = self.expiration_from_ttl(ttl);

        match self.data.entry(key.to_string()) {
            DashEntry::Vacant(entry) => {
                let hash = new_hash();
                hash.insert(field.to_string(), (serialized_value, field_exp));
                let value = StoreValue::Hash(hash, None);
                self.capacity.admit(key, entry_size(key, &value))?;
                entry.insert(value);
                Ok(true)
            }
            DashEntry::Occupied(mut entry) => {
                let value = entry.get_mut();
                match value {
                    StoreValue::Hash(hash, exp) => {
                        if self.is_expired_at(exp, self.get_cached_now()) {
                            let hash = new_hash();
                            hash.insert(field.to_string(), (serialized_value, field_exp));
                            *value = StoreValue::Hash(hash, None);
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                            return Ok(true);
                        }

                        match hash.entry(field.to_string()) {
                            DashEntry::Occupied(mut field) => {
                                field.insert((serialized_value, field_exp));
                                Ok(false)
                            }
                            DashEntry::Vacant(vacant) => {
                                self.capacity
                                    .admit(key, field_size(field, &serialized_value))?;
                                vacant.insert((serialized_value, field_exp));
                                Ok(true)
                            }
                        }
                    }
                    StoreValue::Simple(_, _) | StoreValue::AtomicI64(_, _) => {
                        Err(self.wrong_type_error())
                    }
                }
            }
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

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    let now = self.get_cached_now();
                    if self.is_expired_at(exp, now) {
                        drop(entry);
                        if self.remove_key_if_expired_at(key, now) {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                        return Ok(None);
                    }

                    if let Some(field_entry) = hash_map.get(field) {
                        let (value, field_exp) = field_entry.value();
                        if self.is_expired_at(field_exp, now) {
                            drop(field_entry);
                            if hash_map
                                .remove_if(field, |_, (_, candidate_exp)| {
                                    self.is_expired_at(candidate_exp, now)
                                })
                                .is_some()
                            {
                                self.stats
                                    .expired_entries_cleaned
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                            self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                            return Ok(None);
                        }

                        self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                        let deserialized = self.deserialize(value)?;
                        Ok(Some(deserialized))
                    } else {
                        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                        Ok(None)
                    }
                }
                other => {
                    let now = self.get_cached_now();
                    if self.store_value_is_expired_at(other, now) {
                        drop(entry);
                        self.remove_expired_key(key, now);
                        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                        return Ok(None);
                    }
                    self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                    Err(self.wrong_type_error())
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

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    let now = self.get_cached_now();
                    if self.is_expired_at(exp, now) {
                        drop(entry);
                        if self.remove_key_if_expired_at(key, now) {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        return Ok(false);
                    }
                    Ok(hash_map.remove(field).is_some())
                }
                other => {
                    let now = self.get_cached_now();
                    if self.store_value_is_expired_at(other, now) {
                        drop(entry);
                        self.remove_expired_key(key, now);
                        return Ok(false);
                    }
                    Err(self.wrong_type_error())
                }
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

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    let now = self.get_cached_now();
                    if self.is_expired_at(exp, now) {
                        drop(entry);
                        if self.remove_key_if_expired_at(key, now) {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        return Ok(HashMap::new());
                    }

                    // Pre-allocate HashMap with estimated capacity for better performance
                    let mut result = if self.config.enable_preallocation {
                        HashMap::with_capacity(hash_map.len())
                    } else {
                        HashMap::new()
                    };

                    // Collect expired fields to remove them atomically
                    let mut expired_fields = Vec::new();

                    for entry in hash_map.iter() {
                        let (field, (value, field_exp)) = entry.pair();
                        if field_exp.is_some_and(|exp| exp <= now) {
                            expired_fields.push(field.clone());
                        } else {
                            let deserialized = self.deserialize(value)?;
                            result.insert(field.clone(), deserialized);
                        }
                    }

                    // Remove expired fields
                    for field in expired_fields {
                        if hash_map
                            .remove_if(&field, |_, (_, candidate_exp)| {
                                self.is_expired_at(candidate_exp, now)
                            })
                            .is_some()
                        {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    Ok(result)
                }
                other => {
                    let now = self.get_cached_now();
                    if self.store_value_is_expired_at(other, now) {
                        drop(entry);
                        self.remove_expired_key(key, now);
                        return Ok(HashMap::new());
                    }
                    Err(self.wrong_type_error())
                }
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

        self.stats
            .hash_exists_checks
            .fetch_add(1, Ordering::Relaxed);

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    let now = self.get_cached_now();
                    if self.is_expired_at(exp, now) {
                        drop(entry);
                        if self.remove_key_if_expired_at(key, now) {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        return Ok(false);
                    }

                    if let Some(field_entry) = hash_map.get(field) {
                        let (_, field_exp) = field_entry.value();
                        if self.is_expired_at(field_exp, now) {
                            drop(field_entry);
                            if hash_map
                                .remove_if(field, |_, (_, candidate_exp)| {
                                    self.is_expired_at(candidate_exp, now)
                                })
                                .is_some()
                            {
                                self.stats
                                    .expired_entries_cleaned
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                            Ok(false)
                        } else {
                            Ok(true)
                        }
                    } else {
                        Ok(false)
                    }
                }
                other => {
                    let now = self.get_cached_now();
                    if self.store_value_is_expired_at(other, now) {
                        drop(entry);
                        self.remove_expired_key(key, now);
                        return Ok(false);
                    }
                    Err(self.wrong_type_error())
                }
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

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    let now = self.get_cached_now();
                    if self.is_expired_at(exp, now) {
                        drop(entry);
                        if self.remove_key_if_expired_at(key, now) {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        return Ok(Vec::new());
                    }
                    let mut keys = Vec::new();
                    let mut expired_fields = Vec::new();

                    for entry in hash_map.iter() {
                        let (field, (_, field_exp)) = entry.pair();
                        if field_exp.is_some_and(|exp| exp <= now) {
                            expired_fields.push(field.clone());
                        } else {
                            keys.push(field.clone());
                        }
                    }

                    // Remove expired fields
                    for field in expired_fields {
                        if hash_map
                            .remove_if(&field, |_, (_, candidate_exp)| {
                                self.is_expired_at(candidate_exp, now)
                            })
                            .is_some()
                        {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    Ok(keys)
                }
                other => {
                    let now = self.get_cached_now();
                    if self.store_value_is_expired_at(other, now) {
                        drop(entry);
                        self.remove_expired_key(key, now);
                        return Ok(Vec::new());
                    }
                    Err(self.wrong_type_error())
                }
            }
        } else {
            Ok(Vec::new())
        }
    }

    async fn hvals<T: DeserializeValue>(&self, key: &str) -> StoreResult<Vec<T>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.hash_vals_calls.fetch_add(1, Ordering::Relaxed);

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    let now = self.get_cached_now();
                    if self.is_expired_at(exp, now) {
                        drop(entry);
                        if self.remove_key_if_expired_at(key, now) {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        return Ok(Vec::new());
                    }
                    let mut values = Vec::new();
                    let mut expired_fields = Vec::new();

                    for entry in hash_map.iter() {
                        let (field, (value, field_exp)) = entry.pair();
                        if field_exp.is_some_and(|exp| exp <= now) {
                            expired_fields.push(field.clone());
                        } else {
                            let deserialized = self.deserialize(value)?;
                            values.push(deserialized);
                        }
                    }

                    // Remove expired fields
                    for field in expired_fields {
                        if hash_map
                            .remove_if(&field, |_, (_, candidate_exp)| {
                                self.is_expired_at(candidate_exp, now)
                            })
                            .is_some()
                        {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    Ok(values)
                }
                other => {
                    let now = self.get_cached_now();
                    if self.store_value_is_expired_at(other, now) {
                        drop(entry);
                        self.remove_expired_key(key, now);
                        return Ok(Vec::new());
                    }
                    Err(self.wrong_type_error())
                }
            }
        } else {
            Ok(Vec::new())
        }
    }

    async fn hlen(&self, key: &str) -> StoreResult<usize> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.hash_len_calls.fetch_add(1, Ordering::Relaxed);

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    let now = self.get_cached_now();
                    if self.is_expired_at(exp, now) {
                        drop(entry);
                        if self.remove_key_if_expired_at(key, now) {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        return Ok(0);
                    }

                    // Clean expired fields and count valid ones
                    let mut valid_count = 0;
                    let mut expired_fields = Vec::new();

                    for entry in hash_map.iter() {
                        let (field, (_, field_exp)) = entry.pair();
                        if field_exp.is_some_and(|exp| exp <= now) {
                            expired_fields.push(field.clone());
                        } else {
                            valid_count += 1;
                        }
                    }

                    // Remove expired fields
                    for field in expired_fields {
                        if hash_map
                            .remove_if(&field, |_, (_, candidate_exp)| {
                                self.is_expired_at(candidate_exp, now)
                            })
                            .is_some()
                        {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    Ok(valid_count)
                }
                other => {
                    let now = self.get_cached_now();
                    if self.store_value_is_expired_at(other, now) {
                        drop(entry);
                        self.remove_expired_key(key, now);
                        return Ok(0);
                    }
                    Err(self.wrong_type_error())
                }
            }
        } else {
            Ok(0)
        }
    }

    /// Compare and swap operation for atomic updates
    async fn compare_and_swap<T: SerializeValue>(
        &self,
        key: &str,
        expected: &T,
        new_value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if ttl == Some(0) {
            return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
        }

        let expected_str = self.serialize(expected)?;
        let new_str: Data = self.serialize(new_value)?.into();

        let exp = self.expiration_from_ttl(ttl);

        if let Some(mut entry) = self.data.get_mut(key) {
            match entry.value_mut() {
                StoreValue::Simple(current, current_exp) => {
                    let now = self.get_cached_now();
                    if self.is_expired_at(current_exp, now) {
                        drop(entry);
                        if self.remove_key_if_expired_at(key, now) {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        return Ok(false);
                    }

                    if current.as_ref() == expected_str.as_slice() {
                        *current = new_str;
                        *current_exp = exp;
                        self.stats.sets.fetch_add(1, Ordering::Relaxed);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                _ => Err(self.wrong_type_error()),
            }
        } else {
            Ok(false)
        }
    }

    async fn commit(&self, transaction: &Transaction) -> StoreResult<bool> {
        transaction.validate()?;
        let _applying = self
            .transactions
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let now = self.get_cached_now();
        for requirement in &transaction.requirements {
            if !self.holds(requirement, now)? {
                return Ok(false);
            }
        }
        // One tag per transaction: every key it creates is admitted together,
        // and a refused transaction writes nothing.
        let created = transaction
            .writes
            .iter()
            .filter_map(|write| match write {
                Write::Set(key, value, _) if !self.data.contains_key(key) => {
                    Some((key, slot_size(key) + ARC_HEADER_BYTES + value.len()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some((key, _)) = created.first() {
            self.capacity
                .admit(key, created.iter().map(|(_, bytes)| bytes).sum())?;
        }
        for write in &transaction.writes {
            self.apply(write, now);
        }
        Ok(true)
    }
}

#[async_trait]
impl AtomicStore for MemoryStore {
    async fn get_i64(&self, key: &str) -> StoreResult<Option<i64>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }

        self.stats.gets.fetch_add(1, Ordering::Relaxed);

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::AtomicI64(atomic, exp) => {
                    let now = self.get_cached_now();
                    if self.is_expired_at(exp, now) {
                        drop(entry);
                        if self.remove_key_if_expired_at(key, now) {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                        return Ok(None);
                    }

                    self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                    Ok(Some(atomic.load(Ordering::Relaxed)))
                }
                other => {
                    let now = self.get_cached_now();
                    if self.store_value_is_expired_at(other, now) {
                        drop(entry);
                        self.remove_expired_key(key, now);
                        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                        return Ok(None);
                    }
                    self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
                    Err(self.wrong_type_error())
                }
            }
        } else {
            self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
    }

    async fn set_i64(&self, key: &str, value: i64, ttl: Option<u64>) -> StoreResult<()> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if ttl == Some(0) {
            return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
        }

        let atomic = Arc::new(AtomicI64::new(value));
        let exp = self.expiration_from_ttl(ttl);
        self.insert(key, StoreValue::AtomicI64(atomic, exp))?;
        self.stats.sets.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn incr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if ttl == Some(0) {
            return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
        }

        let exp = self.expiration_from_ttl(ttl);

        let mut entry = self.counter(key)?;

        match entry.value_mut() {
            StoreValue::AtomicI64(atomic, current_exp) => {
                if self.is_expired(current_exp) {
                    // If expired, reset to 'by' value
                    let new_atomic = Arc::new(AtomicI64::new(by));
                    *entry = StoreValue::AtomicI64(new_atomic.clone(), exp);
                    self.stats
                        .expired_entries_cleaned
                        .fetch_add(1, Ordering::Relaxed);
                    self.stats.sets.fetch_add(1, Ordering::Relaxed);
                    return Ok(Some(by));
                }

                let new_value = atomic.fetch_add(by, Ordering::Relaxed) + by;
                // Update expiration if TTL is provided
                if ttl.is_some() {
                    *current_exp = exp;
                }
                self.stats.sets.fetch_add(1, Ordering::Relaxed);
                Ok(Some(new_value))
            }
            _ => Err(self.wrong_type_error()),
        }
    }

    async fn decr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if ttl == Some(0) {
            return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
        }

        let exp = self.expiration_from_ttl(ttl);

        let mut entry = self.counter(key)?;

        match entry.value_mut() {
            StoreValue::AtomicI64(atomic, current_exp) => {
                if self.is_expired(current_exp) {
                    // If expired, reset to -by value
                    let new_atomic = Arc::new(AtomicI64::new(-by));
                    *entry = StoreValue::AtomicI64(new_atomic.clone(), exp);
                    self.stats
                        .expired_entries_cleaned
                        .fetch_add(1, Ordering::Relaxed);
                    self.stats.sets.fetch_add(1, Ordering::Relaxed);
                    return Ok(Some(-by));
                }

                let new_value = atomic.fetch_sub(by, Ordering::Relaxed) - by;
                // Update expiration if TTL is provided
                if ttl.is_some() {
                    *current_exp = exp;
                }
                self.stats.sets.fetch_add(1, Ordering::Relaxed);
                Ok(Some(new_value))
            }
            _ => Err(self.wrong_type_error()),
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
        if ttl == Some(0) {
            return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
        }

        let exp = self.expiration_from_ttl(ttl);

        if let Some(mut entry) = self.data.get_mut(key) {
            match entry.value_mut() {
                StoreValue::AtomicI64(atomic, current_exp) => {
                    let now = self.get_cached_now();
                    if self.is_expired_at(current_exp, now) {
                        // Match Redis semantics: expired key behaves as missing key.
                        drop(entry);
                        if self.remove_key_if_expired_at(key, now) {
                            self.stats
                                .expired_entries_cleaned
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        return Ok(false);
                    }

                    let current_value = atomic.load(Ordering::Relaxed);
                    if current_value == old {
                        atomic.store(new, Ordering::Relaxed);
                        // Update expiration if TTL is provided
                        if ttl.is_some() {
                            *current_exp = exp;
                        }
                        self.stats.sets.fetch_add(1, Ordering::Relaxed);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                _ => Err(self.wrong_type_error()),
            }
        } else {
            Ok(false)
        }
    }
}

impl MemoryStore {
    pub async fn measure_and_set<T, F>(
        &self,
        key: &str,
        default: T,
        measure_fn: F,
        ttl: Option<u64>,
    ) -> StoreResult<(bool, T)>
    where
        T: SerializeValue + DeserializeValue + Clone,
        F: Fn(T) -> (bool, T) + Send + Sync,
    {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        if ttl == Some(0) {
            return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
        }

        let exp = self.expiration_from_ttl(ttl);
        match self.data.entry(key.to_string()) {
            DashEntry::Vacant(vacant) => {
                let (res, new_value) = measure_fn(default);
                let new_data: Data = self.serialize(&new_value)?.into();
                let value = StoreValue::Simple(new_data, exp);
                self.capacity.admit(key, entry_size(key, &value))?;
                vacant.insert(value);
                self.stats.sets.fetch_add(1, Ordering::Relaxed);
                Ok((res, new_value))
            }
            DashEntry::Occupied(mut occupied) => match occupied.get_mut() {
                StoreValue::Simple(current, current_exp) => {
                    if self.is_expired(current_exp) {
                        let (res, new_value) = measure_fn(default.clone());
                        let new_data: Data = self.serialize(&new_value)?.into();
                        *current = new_data;
                        *current_exp = exp;
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::Relaxed);
                        self.stats.sets.fetch_add(1, Ordering::Relaxed);
                        return Ok((res, new_value));
                    }

                    let current_value: T = self.deserialize(current)?;
                    let (res, new_value) = measure_fn(current_value);
                    let new_data: Data = self.serialize(&new_value)?.into();
                    *current = new_data;
                    if ttl.is_some() {
                        *current_exp = exp;
                    }
                    self.stats.sets.fetch_add(1, Ordering::Relaxed);
                    Ok((res, new_value))
                }
                _ => Err(self.wrong_type_error()),
            },
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
        if ttl == Some(0) {
            return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
        }

        let exp = self.expiration_from_ttl(ttl);
        match self.data.entry(key.to_string()) {
            DashEntry::Vacant(vacant) => {
                let (res, new_value) = measure_fn(0);
                let value = StoreValue::AtomicI64(Arc::new(AtomicI64::new(new_value)), exp);
                self.capacity.admit(key, entry_size(key, &value))?;
                vacant.insert(value);
                self.stats.sets.fetch_add(1, Ordering::Relaxed);
                Ok((res, new_value))
            }
            DashEntry::Occupied(mut occupied) => match occupied.get_mut() {
                StoreValue::AtomicI64(atomic, current_exp) => {
                    if self.is_expired(current_exp) {
                        // If expired, reset to measure_fn(0)
                        let (res, new_value) = measure_fn(0);
                        atomic.store(new_value, Ordering::Relaxed);
                        *current_exp = exp;
                        self.stats
                            .expired_entries_cleaned
                            .fetch_add(1, Ordering::Relaxed);
                        self.stats.sets.fetch_add(1, Ordering::Relaxed);
                        return Ok((res, new_value));
                    }

                    let mut current_value = atomic.load(Ordering::Relaxed);
                    let (res, new_value) = loop {
                        let (res, new_value) = measure_fn(current_value);
                        match atomic.compare_exchange_weak(
                            current_value,
                            new_value,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => break (res, new_value),
                            Err(actual) => current_value = actual,
                        }
                    };
                    // Update expiration if TTL is provided
                    if ttl.is_some() {
                        *current_exp = exp;
                    }
                    self.stats.sets.fetch_add(1, Ordering::Relaxed);
                    Ok((res, new_value))
                }
                _ => Err(self.wrong_type_error()),
            },
        }
    }
}

impl MemoryStore {
    /// Batch get operations for better performance - MemoryStore specific.
    ///
    /// Fails on the first invalid key or unreadable value instead of silently
    /// reporting it as missing.
    pub async fn batch_get<T: DeserializeValue>(
        &self,
        keys: &[&str],
    ) -> StoreResult<Vec<Option<T>>> {
        let mut results = Vec::with_capacity(keys.len());

        for &key in keys {
            if key.trim().is_empty() {
                return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
            }

            self.stats.gets.fetch_add(1, Ordering::Relaxed);
            results.push(self.get_if_not_expired(key)?);
        }

        Ok(results)
    }

    /// Batch set operations for better performance - MemoryStore specific.
    ///
    /// Fails on the first invalid key, zero TTL, or unserializable value
    /// instead of silently skipping it.
    pub async fn batch_set<T: SerializeValue>(
        &self,
        operations: &[(&str, &T, Option<u64>)],
    ) -> StoreResult<()> {
        for &(key, value, ttl) in operations {
            if key.trim().is_empty() {
                return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
            }
            if ttl == Some(0) {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }

            let data: Data = self.serialize(value)?.into();
            let exp = self.expiration_from_ttl(ttl);
            self.insert(key, StoreValue::Simple(data, exp))?;
            self.stats.sets.fetch_add(1, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Batch delete operations for better performance - MemoryStore specific
    pub async fn batch_delete(&self, keys: &[&str]) -> StoreResult<Vec<bool>> {
        let mut results = Vec::with_capacity(keys.len());

        for &key in keys {
            if key.trim().is_empty() {
                return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
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
                return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
            }

            self.stats.exists_checks.fetch_add(1, Ordering::Relaxed);

            results.push(self.key_exists_unexpired(key));
        }

        Ok(results)
    }

    /// Removes every expired entry and hash field, then measures the live
    /// entries for the memory bound. Returns how many it removed.
    ///
    /// Reads already treat expired entries as missing; the sweep frees the
    /// memory of those nothing reads again, such as the buckets of clients
    /// that never return.
    pub fn sweep(&self) -> usize {
        let admitted = self.capacity.admitted();
        let now = Utc::now();
        let mut removed = 0;
        let mut measured = 0;
        self.data.retain(|key, value| {
            let live = match value {
                StoreValue::Hash(fields, expiration) => {
                    let before = fields.len();
                    fields.retain(|_, (_, field_expiration)| {
                        field_expiration.is_none_or(|expiration| expiration > now)
                    });
                    removed += before - fields.len();
                    !self.is_expired_at(expiration, now) && !fields.is_empty()
                }
                StoreValue::Simple(_, expiration) | StoreValue::AtomicI64(_, expiration) => {
                    !self.is_expired_at(expiration, now)
                }
            };
            if live {
                measured += entry_size(key, value);
            } else {
                removed += 1;
            }
            live
        });
        self.capacity.measured(measured, admitted);
        self.stats
            .expired_entries_cleaned
            .fetch_add(removed as u64, Ordering::Relaxed);
        removed
    }

    /// Estimated bytes in use, as the memory bound counts them.
    pub fn used_memory_bytes(&self) -> usize {
        self.capacity.used()
    }

    /// Bytes the store may hold before it refuses new entries; zero is
    /// unbounded.
    pub fn memory_limit_bytes(&self) -> usize {
        self.capacity.limit()
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
        let total_keys = self.get_total_keys();
        let memory_usage_bytes = self.get_memory_usage_bytes();

        MemoryStorePerformanceMetrics {
            cache_hit_ratio: self.get_cache_hit_ratio(),
            total_operations,
            total_hash_operations,
            expired_cleanup_efficiency: if total_operations > 0 {
                stats.expired_entries_cleaned as f64 / total_operations as f64
            } else {
                0.0
            },
            memory_usage_bytes,
            total_keys,
            average_key_size: memory_usage_bytes.checked_div(total_keys).unwrap_or(0),
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
                    if self.is_expired_at(exp, now) {
                        expired_keys += 1;
                    }
                }
                StoreValue::Hash(hash_map, exp) => {
                    hash_keys += 1;
                    total_hash_fields += hash_map.len();
                    if self.is_expired_at(exp, now) {
                        expired_keys += 1;
                    } else {
                        // Count expired fields within the hash
                        for field_entry in hash_map.iter() {
                            let (_, (_, field_exp)) = field_entry.pair();
                            if field_exp.is_some_and(|exp| exp <= now) {
                                expired_keys += 1;
                            }
                        }
                    }
                }
                StoreValue::AtomicI64(_, exp) => {
                    atomic_keys += 1;
                    if self.is_expired_at(exp, now) {
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

    /// Whether the store holds more than 90% of its memory limit.
    pub fn is_memory_pressure(&self) -> bool {
        let limit = self.capacity.limit();
        limit > 0 && self.capacity.used() > limit / 10 * 9
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
                "Memory usage is close to the configured limit, beyond which new entries are refused. Consider shorter TTLs or a higher limit.".to_string()
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

    /// Sweeps the store every `interval` on a blocking thread, so expired
    /// entries free their memory and the memory bound sees freed room.
    pub fn run_sweeper(&self, interval: std::time::Duration) {
        let store = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let swept = store.clone();
                match tokio::task::spawn_blocking(move || swept.sweep()).await {
                    Ok(0) => {}
                    Ok(removed) => tracing::debug!(removed, "MemoryStore removed expired entries"),
                    Err(error) => tracing::warn!(%error, "MemoryStore sweep failed"),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[tokio::test]
    async fn test_huge_ttl_saturates_instead_of_panicking() {
        let store = MemoryStore::new();
        store.set("k", &"v", Some(u64::MAX)).await.unwrap();
        let value: Option<String> = store.get("k").await.unwrap();
        assert_eq!(value, Some("v".to_string()));
    }

    #[tokio::test]
    async fn test_compare_and_swap_rejects_zero_ttl() {
        let store = MemoryStore::new();
        store.set("k", &"a", None).await.unwrap();
        let err = store
            .compare_and_swap("k", &"a", &"b", Some(0))
            .await
            .unwrap_err();
        assert!(matches!(err, StoreError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn test_set_if_absent_preserves_existing_value() {
        let store = MemoryStore::new();

        assert!(store.set_if_absent("k", &"first", None).await.unwrap());
        assert!(!store.set_if_absent("k", &"second", None).await.unwrap());
        assert_eq!(
            store.get::<String>("k").await.unwrap().as_deref(),
            Some("first")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_first_hash_writes_preserve_every_field() {
        let store = MemoryStore::new();
        let mut tasks = Vec::new();

        for field in 0..128 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .hset("sessions", &field.to_string(), &field, None)
                    .await
                    .unwrap();
            }));
        }

        for task in tasks {
            task.await.unwrap();
        }

        assert_eq!(store.hlen("sessions").await.unwrap(), 128);
    }

    #[tokio::test]
    async fn test_batch_get_propagates_type_errors() {
        let store = MemoryStore::new();
        store.set("ok", &1_i64, None).await.unwrap();
        store.hset("hash", "f", &1_i64, None).await.unwrap();
        let result: StoreResult<Vec<Option<i64>>> = store.batch_get(&["ok", "hash"]).await;
        assert!(matches!(result, Err(StoreError::TypeMismatch(_))));
    }

    #[tokio::test]
    async fn test_transactions_apply_every_write_or_none() {
        let store = MemoryStore::new();
        let [live, rotation, record] = ["{s}:live", "{s}:rotation", "{s}:record"];
        store.set(live, &"subject", Some(30)).await.unwrap();
        store.set(rotation, &"first", Some(30)).await.unwrap();
        store.set(record, &1, Some(30)).await.unwrap();

        let rotate = |expected: &str| {
            Transaction::new()
                .require(live)
                .require_equal(rotation, &expected)
                .unwrap()
                .set(rotation, &"second", 60)
                .unwrap()
                .swap(record, &1, &2)
                .unwrap()
                .expire(live, 60)
        };
        assert!(!store.commit(&rotate("stale")).await.unwrap());
        assert_eq!(store.get::<i32>(record).await.unwrap(), Some(1));
        assert!(store.commit(&rotate("first")).await.unwrap());
        assert_eq!(
            store.get::<String>(rotation).await.unwrap().as_deref(),
            Some("second")
        );
        assert_eq!(store.get::<i32>(record).await.unwrap(), Some(2));
        let kept = store.expiry(record).unwrap() - Utc::now();
        assert!(kept <= chrono::Duration::seconds(30), "{kept}");
        assert!(store.expiry(live).unwrap() - Utc::now() > chrono::Duration::seconds(30));

        let close = Transaction::new()
            .delete(live)
            .delete(rotation)
            .delete(record);
        assert!(store.commit(&close).await.unwrap());
        assert!(store.commit(&close).await.unwrap());
        assert!(!store.commit(&rotate("second")).await.unwrap());
        for key in [live, rotation, record] {
            assert!(!store.exists(key).await.unwrap(), "{key}");
        }

        store.hset(live, "field", &1, None).await.unwrap();
        let wrong_type = Transaction::new().require_equal(live, &1).unwrap();
        assert!(matches!(
            store.commit(&wrong_type).await,
            Err(StoreError::TypeMismatch(_))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_transactions_are_serialized() {
        let store = MemoryStore::new();
        let [counter, copy] = ["{t}:counter", "{t}:copy"];
        store.set(counter, &0_u32, Some(60)).await.unwrap();
        let mut tasks = Vec::new();
        for _ in 0..32 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                loop {
                    let current = store.get::<u32>(counter).await.unwrap().unwrap();
                    let next = current + 1;
                    let increment = Transaction::new()
                        .swap(counter, &current, &next)
                        .unwrap()
                        .set(copy, &next, 60)
                        .unwrap();
                    if store.commit(&increment).await.unwrap() {
                        break;
                    }
                }
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(store.get::<u32>(counter).await.unwrap(), Some(32));
        assert_eq!(store.get::<u32>(copy).await.unwrap(), Some(32));
    }

    fn bounded(bytes: usize) -> MemoryStore {
        MemoryStore::with_config(MemoryStoreConfig {
            max_memory_usage: bytes,
            ..MemoryStoreConfig::default()
        })
    }

    /// Creates ingress buckets the way the limiter does until the store
    /// refuses one, and returns how many fit.
    async fn flood(store: &MemoryStore, client_prefix: &str) -> usize {
        for index in 0.. {
            let key = crate::keys::limits::ingress("gateway", &format!("{client_prefix}{index}"));
            match store
                .measure_and_set_i64(&key, |tat| (true, tat + 1), Some(1))
                .await
            {
                Ok(_) => continue,
                Err(StoreError::Full) => return index,
                Err(error) => panic!("{error}"),
            }
        }
        unreachable!()
    }

    #[tokio::test]
    async fn a_full_store_stays_under_its_limit_and_keeps_sessions_and_revocations() {
        const LIMIT: usize = 64 * 1024;
        let store = bounded(LIMIT);
        let sid = "01JZ00000000000000000000AA";
        let session = crate::keys::session::subject(sid);
        store.set(&session, &"alice", Some(3600)).await.unwrap();
        store
            .hset(
                &crate::keys::session::index("01JZ00000000000000000000U1"),
                sid,
                &"org",
                Some(3600),
            )
            .await
            .unwrap();

        let admitted = flood(&store, "203.0.113.").await;
        assert!(admitted > 100, "only {admitted} buckets fit");
        assert!(store.used_memory_bytes() <= LIMIT);
        assert!(store.is_memory_pressure());
        // The full measurement agrees: nothing was written beyond the limit.
        assert!(store.get_storage_stats().estimated_memory_bytes <= LIMIT);
        store.sweep();
        assert!(store.used_memory_bytes() <= LIMIT);

        // Nothing was evicted to make room, and existing entries still update.
        assert_eq!(
            store.get::<String>(&session).await.unwrap().as_deref(),
            Some("alice")
        );
        store
            .set(&session, &"alice-renewed", Some(3600))
            .await
            .unwrap();
        let bucket = crate::keys::limits::ingress("gateway", "203.0.113.0");
        store
            .measure_and_set_i64(&bucket, |tat| (true, tat + 1), Some(1))
            .await
            .unwrap();

        // New entries of every kind are refused, except a revocation.
        assert!(matches!(
            store
                .set(
                    &crate::keys::session::subject("01JZ00000000000000000000BB"),
                    &"bob",
                    Some(60)
                )
                .await,
            Err(StoreError::Full)
        ));
        assert!(matches!(
            store
                .incr_i64(&crate::keys::otp::request_ip("198.51.100.1"), 1, Some(60))
                .await,
            Err(StoreError::Full)
        ));
        assert!(matches!(
            store
                .hset(
                    &crate::keys::account::login_known("01JZ00000000000000000000U1"),
                    "198.51.100.1",
                    &1_i64,
                    Some(60)
                )
                .await,
            Err(StoreError::Full)
        ));
        let created = Transaction::new()
            .set(
                &crate::keys::session::record("01JZ00000000000000000000CC"),
                &"record",
                60,
            )
            .unwrap();
        assert!(matches!(
            store.commit(&created).await,
            Err(StoreError::Full)
        ));
        assert!(
            !store
                .exists(&crate::keys::session::record("01JZ00000000000000000000CC"))
                .await
                .unwrap()
        );
        let revoked = crate::keys::session::revoked_jti("jti-1");
        store.set(&revoked, &true, Some(60)).await.unwrap();
        assert_eq!(store.get::<bool>(&revoked).await.unwrap(), Some(true));
        // Callers answer a full store like an unavailable Redis.
        assert!(StoreError::Full.is_unavailable());

        // Once the buckets expire, the sweep frees their room for new clients.
        tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
        assert!(store.sweep() >= admitted);
        assert!(flood(&store, "198.51.100.").await > 100);
        assert!(store.exists(&session).await.unwrap());
        assert!(store.exists(&revoked).await.unwrap());
    }

    #[tokio::test]
    async fn an_unbounded_store_never_refuses() {
        let store = MemoryStore::new();
        for index in 0..10_000 {
            store
                .set(&format!("key-{index}"), &index, Some(60))
                .await
                .unwrap();
        }
        assert_eq!(store.memory_limit_bytes(), 0);
        assert!(!store.is_memory_pressure());
    }

    #[tokio::test]
    async fn test_absorb_replaces_contents() {
        let source = MemoryStore::new();
        source.set("a", &"1", None).await.unwrap();

        let target = MemoryStore::new();
        target.set("b", &"2", None).await.unwrap();

        target.absorb(&source);

        let a: Option<String> = target.get("a").await.unwrap();
        let b: Option<String> = target.get("b").await.unwrap();
        assert_eq!(a, Some("1".to_string()));
        assert_eq!(b, None);
    }
}
