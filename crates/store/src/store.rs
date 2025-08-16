use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

#[async_trait]
pub trait Store: Send + Sync {
    async fn get<T: DeserializeOwned + Send + Sync>(&self, key: &str) -> Option<T>;
    async fn set<T: Serialize + Send + Sync>(&self, key: &str, value: &T, ttl: Option<u64>);
    async fn delete(&self, key: &str) -> bool;
}
