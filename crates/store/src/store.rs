use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;

#[async_trait]
pub trait Store: Send + Sync {
    // Basic key-value operations
    async fn get<T: DeserializeOwned + Send + Sync>(&self, key: &str) -> Option<T>;
    async fn set<T: Serialize + Send + Sync>(&self, key: &str, value: &T, ttl: Option<u64>);
    async fn delete(&self, key: &str) -> bool;
    async fn exists(&self, key: &str) -> bool;

    // Hash operations
    async fn hset<T: Serialize + Send + Sync>(
        &self,
        key: &str,
        field: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> bool;
    async fn hget<T: DeserializeOwned + Send + Sync>(&self, key: &str, field: &str) -> Option<T>;
    async fn hdel(&self, key: &str, field: &str) -> bool;
    async fn hgetall<T: DeserializeOwned + Send + Sync>(&self, key: &str) -> HashMap<String, T>;
    async fn hexists(&self, key: &str, field: &str) -> bool;
    async fn hkeys(&self, key: &str) -> Vec<String>;
    async fn hvals<T: DeserializeOwned + Send + Sync>(&self, key: &str) -> Vec<T>;
    async fn hlen(&self, key: &str) -> usize;
}
