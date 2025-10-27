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
    async fn get<T: DeserializeValue>(&self, key: &str) -> StoreResult<Option<T>>;
    async fn set<T: SerializeValue>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<()>;
    async fn delete(&self, key: &str) -> StoreResult<bool>;
    async fn exists(&self, key: &str) -> StoreResult<bool>;

    // Hash operations
    async fn hset<T: SerializeValue>(
        &self,
        key: &str,
        field: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool>;
    async fn hget<T: DeserializeValue>(&self, key: &str, field: &str) -> StoreResult<Option<T>>;
    async fn hdel(&self, key: &str, field: &str) -> StoreResult<bool>;
    async fn hgetall<T: DeserializeValue>(&self, key: &str) -> StoreResult<HashMap<String, T>>;
    async fn hexists(&self, key: &str, field: &str) -> StoreResult<bool>;
    async fn hkeys(&self, key: &str) -> StoreResult<Vec<String>>;
    async fn hvals<T: DeserializeValue>(&self, key: &str) -> StoreResult<Vec<T>>;
    async fn hlen(&self, key: &str) -> StoreResult<usize>;

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
    async fn get_i64(&self, key: &str) -> StoreResult<Option<i64>>;
    async fn set_i64(&self, key: &str, value: i64, ttl: Option<u64>) -> StoreResult<()>;
    async fn incr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>>;
    async fn decr_i64(&self, key: &str, by: i64, ttl: Option<u64>) -> StoreResult<Option<i64>>;
    async fn compare_and_swap_i64(
        &self,
        key: &str,
        old: i64,
        new: i64,
        ttl: Option<u64>,
    ) -> StoreResult<bool>;
}
