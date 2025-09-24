use std::sync::Arc;

use crate::config::RateLimitConfig;
use crate::error::{RateLimitError, Result};
use crate::quota::{QuotaDecision, QuotaStatus, QuotaTracker};
use store::Store;

/// Manager for handling quota tracking and enforcement
pub struct QuotaManager<S>
where
    S: Store,
{
    store: Arc<S>,
}

impl<S> QuotaManager<S>
where
    S: Store,
{
    /// Create a new quota manager with the given store
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    /// Check quota status for a given key
    pub async fn check_quota(
        &self,
        key: &str,
        count: u64,
        config: &RateLimitConfig,
    ) -> Result<QuotaDecision> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        // If no quota is configured, allow the request
        if !config.has_quota() {
            return Ok(QuotaDecision::allowed(
                QuotaStatus {
                    daily: None,
                    monthly: None,
                    config: crate::quota::QuotaConfig::default(),
                },
                crate::quota::QuotaConfig::default(),
            ));
        }

        // Get or create quota tracker
        let quota_key = format!("quota:{}", key);
        let mut tracker =
            if let Some(tracker) = self.store.hget::<QuotaTracker>("quotas", &quota_key).await {
                tracker
            } else {
                let quota_config = config.quota().unwrap().clone();
                QuotaTracker::new(quota_config)?
            };

        // Check quota
        let decision = tracker.check_quota(count);

        // Save updated tracker
        self.store.hset("quotas", &quota_key, &tracker, None).await;

        Ok(decision)
    }

    /// Consume quota after a successful quota check
    pub async fn consume_quota(&self, key: &str, count: u64) -> Result<()> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        let quota_key = format!("quota:{}", key);
        if let Some(mut tracker) = self.store.hget::<QuotaTracker>("quotas", &quota_key).await {
            tracker.consume_quota(count);
            self.store.hset("quotas", &quota_key, &tracker, None).await;
        }

        Ok(())
    }

    /// Get quota status for a given key
    pub async fn get_quota_status(&self, key: &str) -> Result<Option<QuotaStatus>> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        let quota_key = format!("quota:{}", key);
        if let Some(mut tracker) = self.store.hget::<QuotaTracker>("quotas", &quota_key).await {
            Ok(Some(tracker.get_status()))
        } else {
            Ok(None)
        }
    }

    /// Reset quota for a given key
    pub async fn reset_quota(&self, key: &str) -> Result<bool> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        let quota_key = format!("quota:{}", key);
        let deleted = self.store.hdel("quotas", &quota_key).await;
        Ok(deleted)
    }

    /// Update quota configuration for an existing tracker
    pub async fn update_quota_config(&self, key: &str, config: &RateLimitConfig) -> Result<()> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        let quota_key = format!("quota:{}", key);
        if let Some(mut tracker) = self.store.hget::<QuotaTracker>("quotas", &quota_key).await {
            if let Some(quota_config) = config.quota() {
                tracker.update_config(quota_config.clone())?;
                self.store.hset("quotas", &quota_key, &tracker, None).await;
            }
        }

        Ok(())
    }

    /// Check and consume quota in a single operation
    pub async fn check_and_consume_quota(
        &self,
        key: &str,
        count: u64,
        config: &RateLimitConfig,
    ) -> Result<QuotaDecision> {
        let decision = self.check_quota(key, count, config).await?;

        if decision.allowed {
            self.consume_quota(key, count).await?;
        }

        Ok(decision)
    }

    /// Get multiple quota statuses at once
    pub async fn get_multiple_quota_statuses(
        &self,
        keys: &[&str],
    ) -> Result<Vec<(String, Option<QuotaStatus>)>> {
        let mut results = Vec::new();

        for key in keys {
            let status = self.get_quota_status(key).await?;
            results.push((key.to_string(), status));
        }

        Ok(results)
    }

    /// Bulk reset quotas for multiple keys
    pub async fn reset_multiple_quotas(&self, keys: &[&str]) -> Result<Vec<(String, bool)>> {
        let mut results = Vec::new();

        for key in keys {
            let deleted = self.reset_quota(key).await?;
            results.push((key.to_string(), deleted));
        }

        Ok(results)
    }

    /// Get the store reference
    pub fn store(&self) -> &Arc<S> {
        &self.store
    }

    /// Clean up expired quota trackers
    pub async fn cleanup_expired_trackers(&self) -> Result<usize> {
        // This would require iterating through all quota keys
        // Implementation depends on the store's capabilities for bulk operations
        // For now, return 0 as a placeholder
        Ok(0)
    }
}

impl<S> Clone for QuotaManager<S>
where
    S: Store,
{
    fn clone(&self) -> Self {
        Self {
            store: Arc::clone(&self.store),
        }
    }
}

/// Builder for creating quota managers
pub struct QuotaManagerBuilder<S>
where
    S: Store,
{
    store: Option<Arc<S>>,
}

impl<S> QuotaManagerBuilder<S>
where
    S: Store,
{
    /// Create a new builder
    pub fn new() -> Self {
        Self { store: None }
    }

    /// Set the storage backend
    pub fn store(mut self, store: Arc<S>) -> Self {
        self.store = Some(store);
        self
    }

    /// Set the storage backend from a non-Arc store
    pub fn with_store(mut self, store: S) -> Self {
        self.store = Some(Arc::new(store));
        self
    }

    /// Build the quota manager
    pub fn build(self) -> Result<QuotaManager<S>> {
        let store = self
            .store
            .ok_or_else(|| RateLimitError::internal("store is required"))?;

        Ok(QuotaManager::new(store))
    }
}

impl<S> Default for QuotaManagerBuilder<S>
where
    S: Store,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RateLimitConfig;
    use crate::quota::QuotaConfig;
    use std::sync::Arc;
    use store::MemoryStore;

    #[tokio::test]
    async fn test_quota_manager_basic_functionality() {
        let store = Arc::new(MemoryStore::new());
        let manager = QuotaManager::new(store);

        // Create config with daily quota
        let config = RateLimitConfig::default().with_quota(QuotaConfig::daily_only(100));

        // Should allow requests within quota
        let decision = manager.check_quota("test_key", 10, &config).await.unwrap();
        assert!(decision.allowed);

        // Consume the quota
        manager.consume_quota("test_key", 10).await.unwrap();

        // Check status
        let status = manager.get_quota_status("test_key").await.unwrap().unwrap();
        assert_eq!(status.daily.as_ref().unwrap().used, 10);
        assert_eq!(status.daily.as_ref().unwrap().remaining, 90);
    }

    #[tokio::test]
    async fn test_quota_manager_no_quota_config() {
        let store = Arc::new(MemoryStore::new());
        let manager = QuotaManager::new(store);

        // Config without quota should allow all requests
        let config = RateLimitConfig::default();
        let decision = manager
            .check_quota("test_key", 1000, &config)
            .await
            .unwrap();
        assert!(decision.allowed);

        // Status should be None for keys without quota
        let status = manager.get_quota_status("test_key").await.unwrap();
        assert!(status.is_none());
    }

    #[tokio::test]
    async fn test_quota_manager_check_and_consume() {
        let store = Arc::new(MemoryStore::new());
        let manager = QuotaManager::new(store);

        let config = RateLimitConfig::default().with_quota(QuotaConfig::daily_only(50));

        // Check and consume in one operation
        let decision = manager
            .check_and_consume_quota("test_key", 25, &config)
            .await
            .unwrap();
        assert!(decision.allowed);

        // Status should show consumption
        let status = manager.get_quota_status("test_key").await.unwrap().unwrap();
        assert_eq!(status.daily.as_ref().unwrap().used, 25);
        assert_eq!(status.daily.as_ref().unwrap().remaining, 25);
    }

    #[tokio::test]
    async fn test_quota_manager_exceed_limit() {
        let store = Arc::new(MemoryStore::new());
        let manager = QuotaManager::new(store);

        let config = RateLimitConfig::default().with_quota(QuotaConfig::daily_only(50));

        // Consume most of the quota
        let decision = manager
            .check_and_consume_quota("test_key", 45, &config)
            .await
            .unwrap();
        assert!(decision.allowed);

        // Should deny request that would exceed quota
        let decision = manager.check_quota("test_key", 10, &config).await.unwrap();
        assert!(!decision.allowed);
        assert!(decision.is_daily_violation());
        assert_eq!(decision.used, Some(45));
        assert_eq!(decision.limit, Some(50));
    }

    #[tokio::test]
    async fn test_quota_manager_reset() {
        let store = Arc::new(MemoryStore::new());
        let manager = QuotaManager::new(store);

        let config = RateLimitConfig::default().with_quota(QuotaConfig::daily_only(100));

        // Use some quota
        manager
            .check_and_consume_quota("test_key", 75, &config)
            .await
            .unwrap();

        // Reset quota
        let deleted = manager.reset_quota("test_key").await.unwrap();
        assert!(deleted);

        // Status should be None after reset
        let status = manager.get_quota_status("test_key").await.unwrap();
        assert!(status.is_none());
    }

    #[tokio::test]
    async fn test_quota_manager_multiple_operations() {
        let store = Arc::new(MemoryStore::new());
        let manager = QuotaManager::new(store);

        let config = RateLimitConfig::default().with_quota(QuotaConfig::daily_only(100));

        // Set up some quotas
        manager
            .check_and_consume_quota("key1", 25, &config)
            .await
            .unwrap();
        manager
            .check_and_consume_quota("key2", 50, &config)
            .await
            .unwrap();

        // Get multiple statuses
        let statuses = manager
            .get_multiple_quota_statuses(&["key1", "key2", "key3"])
            .await
            .unwrap();
        assert_eq!(statuses.len(), 3);
        assert!(statuses[0].1.is_some());
        assert!(statuses[1].1.is_some());
        assert!(statuses[2].1.is_none()); // key3 has no quota

        // Reset multiple quotas
        let results = manager
            .reset_multiple_quotas(&["key1", "key2", "key3"])
            .await
            .unwrap();
        assert_eq!(results.len(), 3);
        assert!(results[0].1); // key1 was deleted
        assert!(results[1].1); // key2 was deleted
        assert!(!results[2].1); // key3 didn't exist
    }

    #[tokio::test]
    async fn test_quota_manager_builder() {
        let store = MemoryStore::new();

        let manager = QuotaManagerBuilder::new()
            .with_store(store)
            .build()
            .unwrap();

        let config = RateLimitConfig::default().with_quota(QuotaConfig::daily_only(100));

        let decision = manager.check_quota("test_key", 10, &config).await.unwrap();
        assert!(decision.allowed);
    }

    #[tokio::test]
    async fn test_quota_manager_invalid_key() {
        let store = Arc::new(MemoryStore::new());
        let manager = QuotaManager::new(store);

        let config = RateLimitConfig::default().with_quota(QuotaConfig::daily_only(100));

        // Empty key should return error
        let result = manager.check_quota("", 10, &config).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimitError::InvalidKey { .. }
        ));
    }
}
