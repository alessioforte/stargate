use crate::error::StoreResult;
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;

pub trait SerializeValue: Serialize + Send + Sync {}
impl<T> SerializeValue for T where T: Serialize + Send + Sync {}

pub trait DeserializeValue: DeserializeOwned + Send + Sync {}
impl<T> DeserializeValue for T where T: DeserializeOwned + Send + Sync {}

#[async_trait]
pub trait Store: Send + Sync {
    // Basic key-value operations

    /// Check whether the underlying store backend is healthy and reachable.
    async fn ping(&self) -> StoreResult<()>;

    /// Get a value from the store.
    async fn get<T: DeserializeValue>(&self, key: &str) -> StoreResult<Option<T>>;

    /// Set a value in the store.
    async fn set<T: SerializeValue>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<()>;

    /// Delete a value from the store.
    async fn delete(&self, key: &str) -> StoreResult<bool>;

    /// Check if a key exists in the store.
    async fn exists(&self, key: &str) -> StoreResult<bool>;

    // Hash operations

    /// Set a field in a hash.
    async fn hset<T: SerializeValue>(
        &self,
        key: &str,
        field: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool>;

    /// Get a field from a hash.
    async fn hget<T: DeserializeValue>(&self, key: &str, field: &str) -> StoreResult<Option<T>>;

    /// Delete a field from a hash.
    async fn hdel(&self, key: &str, field: &str) -> StoreResult<bool>;

    /// Get all fields and values from a hash.
    async fn hgetall<T: DeserializeValue>(&self, key: &str) -> StoreResult<HashMap<String, T>>;

    /// Check if a field exists in a hash.
    async fn hexists(&self, key: &str, field: &str) -> StoreResult<bool>;

    /// Get all keys from a hash.
    async fn hkeys(&self, key: &str) -> StoreResult<Vec<String>>;

    /// Get all values from a hash.
    async fn hvals<T: DeserializeValue>(&self, key: &str) -> StoreResult<Vec<T>>;

    /// Get the length of a hash.
    async fn hlen(&self, key: &str) -> StoreResult<usize>;

    /// Compare and swap value
    async fn compare_and_swap<T: SerializeValue + DeserializeValue + PartialEq>(
        &self,
        key: &str,
        expected: &T,
        new: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool>;
}

#[async_trait]
pub trait AtomicStore: Sized {
    // Atomic integer operations

    /// Get the current value of an integer.
    async fn get_i64(&self, key: &str) -> StoreResult<Option<i64>>;

    /// Set the value of an integer.
    async fn set_i64(&self, key: &str, value: i64, ttl: Option<u64>) -> StoreResult<()>;

    /// Increment the value of an integer.
    async fn incr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>>;

    /// Decrement the value of an integer.
    async fn decr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>>;

    /// Compare and swap the value of an integer.
    async fn compare_and_swap_i64(
        &self,
        key: &str,
        old: i64,
        new: i64,
        ttl: Option<u64>,
    ) -> StoreResult<bool>;
}
