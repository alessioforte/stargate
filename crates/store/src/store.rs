use crate::error::{StoreError, StoreResult};
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;

#[async_trait]
pub trait Store: Send + Sync {
    // Basic key-value operations
    async fn get<T: DeserializeOwned + Send + Sync>(&self, key: &str) -> StoreResult<Option<T>>;
    async fn set<T: Serialize + Send + Sync>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<()>;
    async fn delete(&self, key: &str) -> StoreResult<bool>;
    async fn exists(&self, key: &str) -> StoreResult<bool>;

    // Hash operations
    async fn hset<T: Serialize + Send + Sync>(
        &self,
        key: &str,
        field: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool>;
    async fn hget<T: DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
        field: &str,
    ) -> StoreResult<Option<T>>;
    async fn hdel(&self, key: &str, field: &str) -> StoreResult<bool>;
    async fn hgetall<T: DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
    ) -> StoreResult<HashMap<String, T>>;
    async fn hexists(&self, key: &str, field: &str) -> StoreResult<bool>;
    async fn hkeys(&self, key: &str) -> StoreResult<Vec<String>>;
    async fn hvals<T: DeserializeOwned + Send + Sync>(&self, key: &str) -> StoreResult<Vec<T>>;
    async fn hlen(&self, key: &str) -> StoreResult<usize>;

    async fn compare_and_swap<T: Serialize + DeserializeOwned + PartialEq + Send + Sync>(
        &self,
        key: &str,
        expected: &T,
        new: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool>;

    // fn serialize<T: Serialize + Send + Sync>(&self, value: &T) -> StoreResult<Vec<u8>> {
    //     serde_json::to_vec(value)
    //         .map_err(|e| crate::error::StoreError::Serialization(e.to_string()))
    // }

    // fn deserialize<T: DeserializeOwned + Send + Sync>(&self, data: &[u8]) -> StoreResult<T> {
    //     serde_json::from_slice(data)
    //         .map_err(|e| crate::error::StoreError::Deserialization(e.to_string()))
    // }

    fn serialize<T: Serialize + Send + Sync>(&self, value: &T) -> StoreResult<String> {
        serde_json::to_string(value).map_err(|e| {
            StoreError::SerializationFailed(format!("Failed to serialize value: {}", e))
        })
    }

    fn deserialize<T: DeserializeOwned>(&self, data: &String) -> StoreResult<T> {
        serde_json::from_str(data).map_err(|e| {
            StoreError::DeserializationFailed(format!("Failed to deserialize value: {}", e))
        })
    }
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
