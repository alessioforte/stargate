use std::net::IpAddr;
use std::time::Duration;
use store::Store;

use crate::config::{GlobalRateLimitSettings, RateLimitConfig};
use crate::error::{RateLimitDecision, RateLimitError, Result};
use crate::storage::Storage;
use crate::token_bucket::TokenBucket;

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
        custom_config: Option<RateLimitConfig>,
    ) -> Result<RateLimitDecision> {
        if !self.settings.enabled {
            return Ok(RateLimitDecision::allowed(
                f64::MAX,
                f64::MAX,
                self.settings.default_config.clone(),
            ));
        }

        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        // Get or create token bucket
        let mut bucket = self.get_or_create_bucket(key, custom_config).await?;

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
        custom_config: Option<RateLimitConfig>,
    ) -> Result<RateLimitDecision> {
        if !self.settings.enabled {
            return Ok(RateLimitDecision::allowed(
                f64::MAX,
                f64::MAX,
                self.settings.default_config.clone(),
            ));
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
        let mut bucket = self.get_or_create_bucket(key, custom_config).await?;

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
        custom_config: Option<RateLimitConfig>,
    ) -> Result<RateLimitDecision> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        // Get or create token bucket
        let mut bucket = self.get_or_create_bucket(key, custom_config).await?;

        // Check status without consuming tokens
        let available_tokens = bucket.available_tokens();
        let max_tokens = bucket.max_tokens();
        let config = bucket.config().clone();

        Ok(RateLimitDecision::allowed(
            available_tokens,
            max_tokens,
            config,
        ))
    }

    /// Reset the rate limit for a specific key
    pub async fn reset_rate_limit(&self, key: &str) -> Result<bool> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        let deleted = self.store.remove_bucket(key).await?;

        Ok(deleted)
    }

    /// Set a custom configuration for a specific key
    pub async fn set_config(
        &self,
        key: &str,
        config: RateLimitConfig,
        ttl: Option<Duration>,
    ) -> Result<()> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        config
            .validate()
            .map_err(|msg| RateLimitError::invalid_config(&msg))?;

        // Store the configuration
        self.store.set_config(key, config.clone(), ttl).await?;

        // Update existing bucket if it exists
        if let Some(mut bucket) = self.store.get_bucket(key).await? {
            bucket.update_config(config)?;
            self.store.set_bucket(key, bucket).await?;
        }

        Ok(())
    }

    /// Get the configuration for a specific key
    pub async fn get_config(&self, key: &str) -> Result<RateLimitConfig> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        // Try to get custom config first
        if let Some(config) = self.store.get_config(key).await? {
            return Ok(config);
        }

        // Fall back to default config
        Ok(self.settings.default_config.clone())
    }

    /// Remove custom configuration for a specific key
    pub async fn remove_config(&self, key: &str) -> Result<bool> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        // let removed = self.store.hdel("configs", key).await;
        let removed = self.store.remove_config(key).await?;

        // Reset bucket to use default config if it exists
        if let Some(mut bucket) = self.store.get_bucket(key).await? {
            bucket.update_config(self.settings.default_config.clone())?;
            self.store.set_bucket(key, bucket).await?;
        }

        Ok(removed)
    }

    /// Check rate limit for a peer IP address using default IP-based configuration
    /// This is the recommended method when no custom config is provided
    pub async fn check_rate_limit_ip(&self, ip: IpAddr) -> Result<RateLimitDecision> {
        if !self.settings.ip_rate_limiting_enabled {
            return Ok(RateLimitDecision::allowed(
                f64::MAX,
                f64::MAX,
                self.settings.default_ip_config.clone(),
            ));
        }

        let ip_key = Self::format_ip_key(&ip);
        // Use IP-specific default configuration
        self.check_rate_limit(&ip_key, Some(self.settings.default_ip_config.clone()))
            .await
    }

    /// Check rate limit for a peer IP address with custom configuration
    pub async fn check_rate_limit_ip_with_config(
        &self,
        ip: IpAddr,
        custom_config: RateLimitConfig,
    ) -> Result<RateLimitDecision> {
        let ip_key = Self::format_ip_key(&ip);
        self.check_rate_limit(&ip_key, Some(custom_config)).await
    }

    /// Check rate limit for a peer IP address from string representation
    pub async fn check_rate_limit_ip_str(&self, ip_str: &str) -> Result<RateLimitDecision> {
        let ip = ip_str
            .parse::<IpAddr>()
            .map_err(|_| RateLimitError::invalid_key(&format!("Invalid IP address: {}", ip_str)))?;
        self.check_rate_limit_ip(ip).await
    }

    /// Check rate limit for a peer IP address from string with custom configuration
    pub async fn check_rate_limit_ip_str_with_config(
        &self,
        ip_str: &str,
        custom_config: RateLimitConfig,
    ) -> Result<RateLimitDecision> {
        let ip = ip_str
            .parse::<IpAddr>()
            .map_err(|_| RateLimitError::invalid_key(&format!("Invalid IP address: {}", ip_str)))?;
        self.check_rate_limit_ip_with_config(ip, custom_config)
            .await
    }

    /// Check rate limit for multiple tokens for a peer IP address
    pub async fn check_rate_limit_ip_tokens(
        &self,
        ip: IpAddr,
        tokens: f64,
        custom_config: Option<RateLimitConfig>,
    ) -> Result<RateLimitDecision> {
        let ip_key = Self::format_ip_key(&ip);
        self.check_rate_limit_tokens(&ip_key, tokens, custom_config)
            .await
    }

    /// Get the current rate limit status for a peer IP address
    pub async fn get_ip_rate_limit_status(
        &self,
        ip: IpAddr,
        custom_config: Option<RateLimitConfig>,
    ) -> Result<RateLimitDecision> {
        let ip_key = Self::format_ip_key(&ip);
        let config = custom_config.unwrap_or_else(|| self.settings.default_ip_config.clone());
        self.get_rate_limit_status(&ip_key, Some(config)).await
    }

    /// Reset rate limit for a peer IP address
    pub async fn reset_ip_rate_limit(&self, ip: IpAddr) -> Result<bool> {
        let ip_key = Self::format_ip_key(&ip);
        self.reset_rate_limit(&ip_key).await
    }

    /// Set custom configuration for a peer IP address
    pub async fn set_ip_config(
        &self,
        ip: IpAddr,
        config: RateLimitConfig,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let ip_key = Self::format_ip_key(&ip);
        self.set_config(&ip_key, config, ttl).await
    }

    /// Get configuration for a peer IP address
    pub async fn get_ip_config(&self, ip: IpAddr) -> Result<RateLimitConfig> {
        let ip_key = Self::format_ip_key(&ip);

        // Try to get custom config first
        if let Some(config) = self.store.get_config(&ip_key).await? {
            return Ok(config);
        }

        // Fall back to IP-specific default config
        Ok(self.settings.default_ip_config.clone())
    }

    /// Remove custom configuration for a peer IP address
    pub async fn remove_ip_config(&self, ip: IpAddr) -> Result<bool> {
        let ip_key = Self::format_ip_key(&ip);
        self.remove_config(&ip_key).await
    }

    /// Format IP address into a consistent key format for rate limiting
    fn format_ip_key(ip: &IpAddr) -> String {
        format!("ip:{}", ip)
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
    ) -> Result<TokenBucket> {
        // Try to get existing bucket
        if let Some(bucket) = self.store.get_bucket(key).await? {
            return Ok(bucket);
        }

        // Determine configuration to use
        let config = if let Some(config) = custom_config {
            config
        } else if key.starts_with("ip:") {
            // For IP-based keys, use IP-specific default config if no custom config exists
            if let Some(config) = self.store.get_config(key).await? {
                config
            } else {
                self.settings.default_ip_config.clone()
            }
        } else {
            self.get_config(key).await?
        };

        // Create new bucket
        let bucket = TokenBucket::new(config)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use store::MemoryStore;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_rate_limiter_basic_functionality() {
        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        // First request should be allowed
        let decision = limiter.check_rate_limit("test_key", None).await.unwrap();
        assert!(decision.allowed);

        // Should be able to make multiple requests up to burst limit
        for _ in 0..19 {
            let decision = limiter.check_rate_limit("test_key", None).await.unwrap();
            assert!(decision.allowed);
        }

        // Should be rate limited after burst
        let decision = limiter.check_rate_limit("test_key", None).await.unwrap();
        assert!(!decision.allowed);
        assert!(decision.retry_after_ms.is_some());
    }

    #[tokio::test]
    async fn test_rate_limiter_with_custom_config() {
        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        let custom_config = RateLimitConfig::new(1000, 2000, 60); // Very permissive

        // Should allow many requests with custom config
        for _ in 0..100 {
            let decision = limiter
                .check_rate_limit("test_key", Some(custom_config.clone()))
                .await
                .unwrap();
            assert!(decision.allowed);
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_token_consumption() {
        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        // Consume 10 tokens at once
        let decision = limiter
            .check_rate_limit_tokens("test_key", 10.0, None)
            .await
            .unwrap();
        assert!(decision.allowed);
        assert_eq!(decision.remaining_tokens, 10.0); // 20 - 10 = 10

        // Try to consume 15 more tokens (should fail)
        let decision = limiter
            .check_rate_limit_tokens("test_key", 15.0, None)
            .await
            .unwrap();
        assert!(!decision.allowed);
    }

    #[tokio::test]
    async fn test_rate_limiter_status_check() {
        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        // Check initial status
        let status = limiter
            .get_rate_limit_status("test_key", None)
            .await
            .unwrap();
        assert_eq!(status.remaining_tokens, 20.0);

        // Consume some tokens
        limiter.check_rate_limit("test_key", None).await.unwrap();

        // Check status again
        let status = limiter
            .get_rate_limit_status("test_key", None)
            .await
            .unwrap();
        assert_eq!(status.remaining_tokens, 19.0);
    }

    #[tokio::test]
    async fn test_rate_limiter_config_management() {
        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        let custom_config = RateLimitConfig::new(50, 100, 60);

        // Set custom config
        limiter
            .set_config("test_key", custom_config.clone(), None)
            .await
            .unwrap();

        // Get config should return the custom one
        let retrieved_config = limiter.get_config("test_key").await.unwrap();
        assert_eq!(retrieved_config, custom_config);

        // Remove config
        let removed = limiter.remove_config("test_key").await.unwrap();
        assert!(removed);

        // Should now return default config
        let default_config = limiter.get_config("test_key").await.unwrap();
        assert_eq!(default_config, RateLimitConfig::default());
    }

    #[tokio::test]
    async fn test_rate_limiter_reset() {
        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        // Exhaust the rate limit
        for _ in 0..20 {
            limiter.check_rate_limit("test_key", None).await.unwrap();
        }

        // Should be rate limited
        let decision = limiter.check_rate_limit("test_key", None).await.unwrap();
        assert!(!decision.allowed);

        // Reset the rate limit
        let reset = limiter.reset_rate_limit("test_key").await.unwrap();
        assert!(reset);

        // Should be allowed again
        let decision = limiter.check_rate_limit("test_key", None).await.unwrap();
        assert!(decision.allowed);
    }

    #[tokio::test]
    async fn test_rate_limiter_builder() {
        let store = MemoryStore::new();
        let custom_config = RateLimitConfig::new(100, 200, 60);

        let limiter = RateLimiterBuilder::new()
            .store(store)
            .default_config(custom_config.clone())
            .enabled(true)
            .cleanup_interval(Duration::from_secs(600))
            .max_buckets(5000)
            .build()
            .unwrap();

        assert_eq!(limiter.settings().default_config, custom_config);
        assert!(limiter.settings().enabled);
        assert_eq!(limiter.settings().cleanup_interval_seconds, 600);
        assert_eq!(limiter.settings().max_buckets, 5000);
    }

    #[tokio::test]
    async fn test_rate_limiter_disabled() {
        let store = MemoryStore::new();
        let settings = GlobalRateLimitSettings {
            enabled: false,
            ..Default::default()
        };

        let limiter = RateLimiter::new(store, settings);

        // All requests should be allowed when disabled
        for _ in 0..1000 {
            let decision = limiter.check_rate_limit("test_key", None).await.unwrap();
            assert!(decision.allowed);
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_with_ttl_config() {
        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        let custom_config = RateLimitConfig::new(50, 100, 60);

        // Set config with TTL
        limiter
            .set_config(
                "test_key",
                custom_config.clone(),
                Some(Duration::from_secs(1)),
            )
            .await
            .unwrap();

        // Should have custom config
        let config = limiter.get_config("test_key").await.unwrap();
        assert_eq!(config, custom_config);

        // Wait for expiration
        sleep(Duration::from_secs(2)).await;

        // Should fall back to default config
        let config = limiter.get_config("test_key").await.unwrap();
        assert_eq!(config, RateLimitConfig::default());
    }

    #[tokio::test]
    async fn test_ip_rate_limiting() {
        use std::net::Ipv4Addr;

        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));

        // First request should be allowed
        let decision = limiter.check_rate_limit_ip(ip).await.unwrap();
        assert!(decision.allowed);
        assert_eq!(decision.remaining_tokens, 19.0); // 20 - 1 = 19

        // Should be able to make multiple requests up to burst limit
        for _ in 0..19 {
            let decision = limiter.check_rate_limit_ip(ip).await.unwrap();
            assert!(decision.allowed);
        }

        // Should be rate limited after burst
        let decision = limiter.check_rate_limit_ip(ip).await.unwrap();
        assert!(!decision.allowed);
        assert!(decision.retry_after_ms.is_some());
    }

    #[tokio::test]
    async fn test_ip_rate_limiting_with_custom_config() {
        use std::net::Ipv6Addr;

        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        let custom_config = RateLimitConfig::new(100, 200, 60);

        // Should allow many requests with custom config
        for _ in 0..50 {
            let decision = limiter
                .check_rate_limit_ip_with_config(ip, custom_config.clone())
                .await
                .unwrap();
            assert!(decision.allowed);
        }
    }

    #[tokio::test]
    async fn test_ip_rate_limiting_from_string() {
        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        // Test valid IP string
        let decision = limiter.check_rate_limit_ip_str("10.0.0.1").await.unwrap();
        assert!(decision.allowed);

        // Test invalid IP string
        let result = limiter.check_rate_limit_ip_str("invalid_ip").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimitError::InvalidKey { .. }
        ));
    }

    #[tokio::test]
    async fn test_ip_rate_limiting_token_consumption() {
        use std::net::Ipv4Addr;

        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        let ip = IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1));

        // Consume multiple tokens at once
        let decision = limiter
            .check_rate_limit_ip_tokens(ip, 5.0, None)
            .await
            .unwrap();
        assert!(decision.allowed);
        assert_eq!(decision.remaining_tokens, 15.0); // 20 - 5 = 15

        // Try to consume more tokens than available
        let decision = limiter
            .check_rate_limit_ip_tokens(ip, 20.0, None)
            .await
            .unwrap();
        assert!(!decision.allowed);
    }

    #[tokio::test]
    async fn test_ip_config_management() {
        use std::net::Ipv4Addr;

        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
        let custom_config = RateLimitConfig::new(50, 100, 60);

        // Set custom config for IP
        limiter
            .set_ip_config(ip, custom_config.clone(), None)
            .await
            .unwrap();

        // Get config should return the custom one
        let retrieved_config = limiter.get_ip_config(ip).await.unwrap();
        assert_eq!(retrieved_config, custom_config);

        // Remove config
        let removed = limiter.remove_ip_config(ip).await.unwrap();
        assert!(removed);

        // Should now return default config
        let default_config = limiter.get_ip_config(ip).await.unwrap();
        assert_eq!(default_config, RateLimitConfig::default());
    }

    #[tokio::test]
    async fn test_ip_key_formatting() {
        use std::net::{Ipv4Addr, Ipv6Addr};

        let ipv4 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let ipv6 = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));

        assert_eq!(
            RateLimiter::<MemoryStore>::format_ip_key(&ipv4),
            "ip:192.168.1.1"
        );
        assert_eq!(
            RateLimiter::<MemoryStore>::format_ip_key(&ipv6),
            "ip:2001:db8::1"
        );
    }

    #[tokio::test]
    async fn test_separate_ip_rate_limits() {
        use std::net::Ipv4Addr;

        let store = MemoryStore::new();
        let limiter = RateLimiter::with_store(store);

        let ip1 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        let ip2 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 101));

        // Exhaust rate limit for first IP
        for _ in 0..20 {
            limiter.check_rate_limit_ip(ip1).await.unwrap();
        }

        // First IP should be rate limited
        let decision = limiter.check_rate_limit_ip(ip1).await.unwrap();
        assert!(!decision.allowed);

        // Second IP should still be allowed
        let decision = limiter.check_rate_limit_ip(ip2).await.unwrap();
        assert!(decision.allowed);
    }
}
