use crate::config::RateLimitConfig;
use crate::error::{RateLimitError, Result};
use crate::quota::QuotaTracker;
use crate::token_bucket::TokenBucket;
use std::sync::Arc;
use std::time::Duration;
use store::Store;

pub struct Storage<S>
where
    S: Store,
{
    store: Arc<S>,
}

impl<S> Storage<S>
where
    S: Store,
{
    pub fn new(store: S) -> Self {
        Self {
            store: Arc::new(store),
        }
    }

    /// Get a token bucket for the given key
    pub async fn get_bucket(&self, key: &str) -> Result<Option<TokenBucket>> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self.store.hget("buckets", &key).await {
            Ok(bucket) => Ok(bucket),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }

    /// Store a token bucket for the given key
    pub async fn set_bucket(&self, key: &str, bucket: TokenBucket) -> Result<()> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self.store.hset("buckets", &key, &bucket, None).await {
            Ok(_) => Ok(()),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }

    /// Remove a token bucket for the given key
    pub async fn remove_bucket(&self, key: &str) -> Result<bool> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self.store.hdel("buckets", &key).await {
            Ok(removed) => Ok(removed),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }

    /// Get rate limit configuration for the given key
    pub async fn get_config(&self, key: &str) -> Result<Option<RateLimitConfig>> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self.store.hget("configs", &key).await {
            Ok(config) => Ok(config),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }

    /// Store rate limit configuration for the given key
    pub async fn set_config(
        &self,
        key: &str,
        config: RateLimitConfig,
        ttl: Option<Duration>,
    ) -> Result<()> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self
            .store
            .hset("configs", &key, &config, ttl.map(|d| d.as_secs()))
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }

    /// Remove rate limit configuration for the given key
    pub async fn remove_config(&self, key: &str) -> Result<bool> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self.store.hdel("configs", &key).await {
            Ok(removed) => Ok(removed),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }

    /// Get quota usage for the given key
    pub async fn get_quota(&self, key: &str) -> Result<Option<QuotaTracker>> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self.store.hget("quotas", &key).await {
            Ok(quota) => Ok(quota),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }

    /// Store quota usage for the given key
    pub async fn set_quota(&self, key: &str, quota: QuotaTracker) -> Result<()> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self.store.hset("quotas", &key, &quota, None).await {
            Ok(_) => Ok(()),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }

    /// Remove quota usage for the given key
    pub async fn remove_quota(&self, key: &str) -> Result<bool> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self.store.hdel("quotas", &key).await {
            Ok(removed) => Ok(removed),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }
}

impl<S> Clone for Storage<S>
where
    S: Store,
{
    fn clone(&self) -> Self {
        Self {
            store: Arc::clone(&self.store),
        }
    }
}
