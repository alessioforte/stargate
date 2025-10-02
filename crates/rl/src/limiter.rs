use std::time::Duration;
use store::Store;

use crate::bucket::Bucket;
use crate::config::{GlobalRateLimitSettings, RateLimitConfig};
use crate::decision::{QuotaDecision, RateLimitDecision};
use crate::error::{RateLimitError, Result};
use crate::storage::Storage;

/// Main rate limiter that manages token buckets and configurations
pub struct RateLimiter<S>
where
    S: Store,
{
    store: Storage<S>,
    settings: GlobalRateLimitSettings,
}

impl<S> RateLimiter<S>
where
    S: Store,
{
    /// Create a new rate limiter with the given store and settings
    pub fn new(store: S, settings: GlobalRateLimitSettings) -> Self {
        Self {
            store: Storage::new(store),
            settings,
        }
    }

    /// Create a new rate limiter with default settings
    pub fn with_store(store: S) -> Self {
        Self::new(store, GlobalRateLimitSettings::default())
    }

    /// Check if a request is allowed for the given key
    pub async fn check_rate_limit(
        &self,
        key: &str,
        config: Option<RateLimitConfig>,
    ) -> Result<RateLimitDecision> {
        if !self.settings.enabled {
            return Ok(RateLimitDecision::allowed(f64::MAX, f64::MAX));
        }

        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        // Get or create token bucket
        let mut bucket = self.get_or_create_bucket(key, config).await?;

        // Try to consume one token
        let decision = bucket.try_consume(1.0);

        // Store the updated bucket back
        self.store.set_bucket(&key, bucket).await?;

        Ok(decision)
    }

    /// Check if multiple tokens can be consumed
    pub async fn check_rate_limit_tokens(
        &self,
        key: &str,
        tokens: f64,
        config: Option<RateLimitConfig>,
    ) -> Result<RateLimitDecision> {
        if !self.settings.enabled {
            return Ok(RateLimitDecision::allowed(f64::MAX, f64::MAX));
        }

        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        if tokens <= 0.0 {
            return Err(RateLimitError::invalid_config(
                "tokens must be greater than 0",
            ));
        }

        // Get or create token bucket
        let mut bucket = self.get_or_create_bucket(key, config).await?;

        // Try to consume the requested tokens
        let decision = bucket.try_consume(tokens);

        // Store the updated bucket back
        self.store.set_bucket(&key, bucket).await?;

        Ok(decision)
    }

    /// Get the current status of a rate limit key without consuming tokens
    pub async fn get_rate_limit_status(
        &self,
        key: &str,
        config: Option<RateLimitConfig>,
    ) -> Result<RateLimitDecision> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        // Get or create token bucket
        let mut bucket = self.get_or_create_bucket(key, config).await?;

        // Check status without consuming tokens
        let available_tokens = bucket.available_tokens();
        let max_tokens = bucket.max_tokens();

        Ok(RateLimitDecision::allowed(available_tokens, max_tokens))
    }

    /// Reset the rate limit for a specific key
    pub async fn reset_rate_limit(&self, key: &str) -> Result<bool> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        let deleted = self.store.remove_bucket(key).await?;

        Ok(deleted)
    }

    pub async fn check_rate_limit_and_consume_quota(
        &self,
        key: &str,
        count: u64,
        config: Option<RateLimitConfig>,
    ) -> Result<(RateLimitDecision, QuotaDecision)> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        // Get or create token bucket
        let mut bucket = self.get_or_create_bucket(key, config).await?;

        // Use optimized single-timestamp method
        let (rate_limit_decision, quota_decision) = bucket.check_rate_limit_and_quota(1.0, count);

        // Store the updated bucket back
        self.store.set_bucket(&key, bucket).await?;

        Ok((rate_limit_decision, quota_decision))
    }

    /// Set a custom configuration for a specific key if the bucket exists
    /// If the bucket does not exist, this is a no-op
    /// To create a bucket with custom config, call `check_rate_limit` first
    pub async fn set_config(&self, key: &str, config: RateLimitConfig) -> Result<()> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        config
            .validate()
            .map_err(|msg| RateLimitError::invalid_config(&msg))?;

        // Update existing bucket if it exists
        if let Some(mut bucket) = self.store.get_bucket(key).await? {
            bucket.update_config(config)?;
            self.store.set_bucket(key, bucket).await?;
        }

        Ok(())
    }

    /// Get the current settings
    pub fn settings(&self) -> &GlobalRateLimitSettings {
        &self.settings
    }

    /// Update the global settings
    pub fn update_settings(&mut self, settings: GlobalRateLimitSettings) {
        self.settings = settings;
    }

    /// Get or create a token bucket for the given key
    async fn get_or_create_bucket(
        &self,
        key: &str,
        custom_config: Option<RateLimitConfig>,
    ) -> Result<Bucket> {
        // Try to get existing bucket
        if let Some(bucket) = self.store.get_bucket(key).await? {
            return Ok(bucket);
        }

        let config = if let Some(config) = custom_config {
            config
        } else {
            self.settings.default_config.clone()
        };

        // Create new bucket
        let bucket = Bucket::new(config)?;
        Ok(bucket)
    }
}

impl<S> Clone for RateLimiter<S>
where
    S: Store,
{
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            settings: self.settings.clone(),
        }
    }
}

/// Builder for creating rate limiters with custom settings
pub struct RateLimiterBuilder<S>
where
    S: Store,
{
    store: Option<S>,
    settings: GlobalRateLimitSettings,
}

impl<S> RateLimiterBuilder<S>
where
    S: Store,
{
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            store: None,
            settings: GlobalRateLimitSettings::default(),
        }
    }

    /// Set the storage backend
    pub fn store(mut self, store: S) -> Self {
        self.store = Some(store);
        self
    }

    /// Set the default rate limit configuration
    pub fn default_config(mut self, config: RateLimitConfig) -> Self {
        self.settings.default_config = config;
        self
    }

    /// Enable or disable rate limiting globally
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.settings.enabled = enabled;
        self
    }

    /// Set the cleanup interval
    pub fn cleanup_interval(mut self, interval: Duration) -> Self {
        self.settings.cleanup_interval_seconds = interval.as_secs();
        self
    }

    /// Set the maximum number of buckets to keep in memory
    pub fn max_buckets(mut self, max_buckets: usize) -> Self {
        self.settings.max_buckets = max_buckets;
        self
    }

    /// Set the configuration cache TTL
    pub fn config_cache_ttl(mut self, ttl: Duration) -> Self {
        self.settings.config_cache_ttl_seconds = ttl.as_secs();
        self
    }

    /// Set the complete global settings
    pub fn settings(mut self, settings: GlobalRateLimitSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Build the rate limiter
    pub fn build(self) -> Result<RateLimiter<S>> {
        let store = self
            .store
            .ok_or_else(|| RateLimitError::internal("store is required"))?;

        self.settings
            .default_config
            .validate()
            .map_err(|msg| RateLimitError::invalid_config(&msg))?;

        Ok(RateLimiter::new(store, self.settings))
    }
}

impl<S> Default for RateLimiterBuilder<S>
where
    S: Store,
{
    fn default() -> Self {
        Self::new()
    }
}
