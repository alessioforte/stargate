use crate::bucket::Bucket;
use crate::error::{RateLimitError, Result};
use std::sync::Arc;
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
    pub async fn get_bucket(&self, key: &str) -> Result<Option<Bucket>> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self.store.get(format!("lim:{}", key).as_str()).await {
            Ok(bucket) => Ok(bucket),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }

    /// Store a token bucket for the given key
    pub async fn set_bucket(&self, key: &str, bucket: Bucket) -> Result<()> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self
            .store
            .set(format!("lim:{}", key).as_str(), &bucket, None)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }

    /// Remove a token bucket for the given key
    pub async fn remove_bucket(&self, key: &str) -> Result<bool> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        match self.store.delete(format!("lim:{}", key).as_str()).await {
            Ok(removed) => Ok(removed),
            Err(e) => Err(RateLimitError::storage(&e.to_string())),
        }
    }

    pub async fn compare_and_swap_bucket(
        &self,
        key: &str,
        old: &Bucket,
        new: &Bucket,
    ) -> Result<bool> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        let store_key = format!("lim:{}", key);

        match self
            .store
            .compare_and_swap(&store_key, old, new, None)
            .await
        {
            Ok(swapped) => Ok(swapped),
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
