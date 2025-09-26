use crate::error::StoreResult;
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
}
