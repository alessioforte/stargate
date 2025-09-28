use std::net::IpAddr;
use std::time::Duration;

use crate::config::{GlobalRateLimitSettings, RateLimitConfig};
use crate::error::{RateLimitDecision, RateLimitError, Result};
use crate::limiter::RateLimiter;
use crate::quota::{QuotaDecision, QuotaStatus, ResetPeriod};
use crate::quota_manager::QuotaManager;
use store::Store;

/// Combined decision from both rate limiting and quota checks
#[derive(Debug, Clone)]
pub struct UnifiedDecision {
    /// Rate limiting decision
    pub rate_limit: RateLimitDecision,
    /// Quota decision (None if rate limiting failed or no quota configured)
    pub quota: Option<QuotaDecision>,
    /// Whether the overall request is allowed (both rate limit and quota must pass)
    pub allowed: bool,
    /// The primary reason for denial (rate limit or quota)
    pub denial_reason: Option<DenialReason>,
}

/// Reason why a request was denied
#[derive(Debug, Clone, PartialEq)]
pub enum DenialReason {
    /// Rate limit exceeded
    RateLimit,
    /// Quota exceeded for a specific reset period
    Quota(ResetPeriod),
}

impl UnifiedDecision {
    /// Create an allowed decision
    pub fn allowed(rate_limit: RateLimitDecision, quota: Option<QuotaDecision>) -> Self {
        Self {
            rate_limit,
            quota,
            allowed: true,
            denial_reason: None,
        }
    }

    /// Create a denied decision
    pub fn denied(
        rate_limit: RateLimitDecision,
        quota: Option<QuotaDecision>,
        reason: DenialReason,
    ) -> Self {
        Self {
            rate_limit,
            quota,
            allowed: false,
            denial_reason: Some(reason),
        }
    }

    /// Get the retry after time in milliseconds (from rate limit or quota)
    pub fn retry_after_ms(&self) -> Option<u64> {
        if let Some(rate_retry) = self.rate_limit.retry_after_ms {
            if !self.rate_limit.allowed {
                return Some(rate_retry);
            }
        }

        if let Some(quota) = &self.quota {
            if let Some(quota_retry) = quota.retry_after {
                return quota_retry.to_std().ok().map(|d| d.as_millis() as u64);
            }
        }

        None
    }

    /// Check if the denial was due to rate limiting
    pub fn is_rate_limit_violation(&self) -> bool {
        matches!(self.denial_reason, Some(DenialReason::RateLimit))
    }

    /// Check if the denial was due to quota violation
    /// Check if this is a quota violation
    pub fn is_quota_violation(&self) -> bool {
        matches!(self.denial_reason, Some(DenialReason::Quota(_)))
    }

    /// Check if this is a daily quota violation
    pub fn is_daily_quota_violation(&self) -> bool {
        matches!(
            self.denial_reason,
            Some(DenialReason::Quota(ResetPeriod::Daily))
        )
    }

    /// Check if this is a monthly quota violation
    pub fn is_monthly_quota_violation(&self) -> bool {
        matches!(
            self.denial_reason,
            Some(DenialReason::Quota(ResetPeriod::Monthly))
        )
    }

    /// Check if this is a weekly quota violation
    pub fn is_weekly_quota_violation(&self) -> bool {
        matches!(
            self.denial_reason,
            Some(DenialReason::Quota(ResetPeriod::Weekly))
        )
    }

    /// Check if this is a yearly quota violation
    pub fn is_yearly_quota_violation(&self) -> bool {
        matches!(
            self.denial_reason,
            Some(DenialReason::Quota(ResetPeriod::Yearly))
        )
    }

    /// Check if this is a permanent quota violation
    pub fn is_permanent_quota_violation(&self) -> bool {
        matches!(
            self.denial_reason,
            Some(DenialReason::Quota(ResetPeriod::Never))
        )
    }
}

/// Unified service that combines rate limiting and quota management
pub struct UnifiedLimiter<S>
where
    S: Store,
{
    rate_limiter: RateLimiter<S>,
    quota_manager: QuotaManager<S>,
}

impl<S> UnifiedLimiter<S>
where
    S: Store,
{
    /// Create a new unified limiter
    pub fn new(rate_limiter: RateLimiter<S>, quota_manager: QuotaManager<S>) -> Self {
        Self {
            rate_limiter,
            quota_manager,
        }
    }

    /// Create a new unified limiter from store and settings
    pub fn from_store(store: S, settings: GlobalRateLimitSettings) -> Self
    where
        S: Clone,
    {
        let store_clone = store.clone();
        let rate_limiter = RateLimiter::new(store, settings);
        let quota_manager = QuotaManager::new(store_clone);
        Self::new(rate_limiter, quota_manager)
    }

    /// Create a new unified limiter with separate components
    pub fn with_components(rate_limiter: RateLimiter<S>, quota_manager: QuotaManager<S>) -> Self {
        Self {
            rate_limiter,
            quota_manager,
        }
    }

    /// Create a unified limiter with default settings
    pub fn with_store(store: S) -> Self
    where
        S: Clone,
    {
        Self::from_store(store, GlobalRateLimitSettings::default())
    }

    /// Check both rate limit and quota for a request
    pub async fn check_request(
        &self,
        key: &str,
        cost: u64,
        custom_config: Option<RateLimitConfig>,
    ) -> Result<UnifiedDecision> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        // First check rate limiting
        let rate_decision = self
            .rate_limiter
            .check_rate_limit_tokens(key, cost as f64, custom_config.clone())
            .await?;

        // If rate limiting fails, return early
        if !rate_decision.allowed {
            return Ok(UnifiedDecision::denied(
                rate_decision,
                None,
                DenialReason::RateLimit,
            ));
        }

        // Get configuration for quota check
        let config = if let Some(config) = custom_config {
            config
        } else {
            self.rate_limiter.get_config(key).await?
        };

        // Check quota if configured
        if config.has_quota() {
            let quota_decision = self.quota_manager.check_quota(key, cost, &config).await?;

            if !quota_decision.allowed {
                let reason = match &quota_decision.violation_type {
                    Some(crate::quota::QuotaViolationType::Quota(period)) => {
                        DenialReason::Quota(period.clone())
                    }
                    None => DenialReason::Quota(ResetPeriod::Daily), // fallback
                };

                return Ok(UnifiedDecision::denied(
                    rate_decision,
                    Some(quota_decision),
                    reason,
                ));
            }

            Ok(UnifiedDecision::allowed(
                rate_decision,
                Some(quota_decision),
            ))
        } else {
            Ok(UnifiedDecision::allowed(rate_decision, None))
        }
    }

    /// Check and consume both rate limit tokens and quota
    pub async fn check_and_consume(
        &self,
        key: &str,
        cost: u64,
        custom_config: Option<RateLimitConfig>,
    ) -> Result<UnifiedDecision> {
        let decision = self.check_request(key, cost, custom_config).await?;

        // Only consume quota if everything is allowed
        if decision.allowed {
            if let Some(_) = decision.quota {
                self.quota_manager.consume_quota(key, cost).await?;
            }
        }

        Ok(decision)
    }

    /// Check request for IP address
    pub async fn check_ip_request(&self, ip: IpAddr, cost: u64) -> Result<UnifiedDecision> {
        let key = format!("ip:{}", ip);
        let config = self.rate_limiter.get_ip_config(ip).await?;
        self.check_request(&key, cost, Some(config)).await
    }

    /// Check and consume for IP address
    pub async fn check_and_consume_ip(&self, ip: IpAddr, cost: u64) -> Result<UnifiedDecision> {
        let key = format!("ip:{}", ip);
        let config = self.rate_limiter.get_ip_config(ip).await?;
        self.check_and_consume(&key, cost, Some(config)).await
    }

    /// Get comprehensive status for a key (both rate limit and quota)
    pub async fn get_status(
        &self,
        key: &str,
        custom_config: Option<RateLimitConfig>,
    ) -> Result<LimiterStatus> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        let rate_status = self
            .rate_limiter
            .get_rate_limit_status(key, custom_config)
            .await?;

        let quota_status = self.quota_manager.get_quota_status(key).await?;

        Ok(LimiterStatus {
            rate_limit: rate_status,
            quota: quota_status,
        })
    }

    /// Get status for IP address
    pub async fn get_ip_status(&self, ip: IpAddr) -> Result<LimiterStatus> {
        let key = format!("ip:{}", ip);
        let config = self.rate_limiter.get_ip_config(ip).await?;
        self.get_status(&key, Some(config)).await
    }

    /// Reset both rate limit and quota for a key
    pub async fn reset_all(&self, key: &str) -> Result<ResetResult> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        let rate_reset = self.rate_limiter.reset_rate_limit(key).await?;
        let quota_reset = self.quota_manager.reset_quota(key).await?;

        Ok(ResetResult {
            rate_limit_reset: rate_reset,
            quota_reset,
        })
    }

    /// Reset both rate limit and quota for IP address
    pub async fn reset_ip_all(&self, ip: IpAddr) -> Result<ResetResult> {
        let key = format!("ip:{}", ip);
        self.reset_all(&key).await
    }

    /// Set configuration for a key and update existing trackers
    pub async fn set_config(
        &self,
        key: &str,
        config: RateLimitConfig,
        ttl: Option<Duration>,
    ) -> Result<()> {
        if key.is_empty() {
            return Err(RateLimitError::invalid_key(key));
        }

        // Set config in rate limiter (this handles bucket updates)
        self.rate_limiter
            .set_config(key, config.clone(), ttl)
            .await?;

        // Update quota configuration if it exists
        self.quota_manager.update_quota_config(key, &config).await?;

        Ok(())
    }

    /// Set configuration for IP address
    pub async fn set_ip_config(
        &self,
        ip: IpAddr,
        config: RateLimitConfig,
        ttl: Option<Duration>,
    ) -> Result<()> {
        self.rate_limiter.set_ip_config(ip, config, ttl).await
    }

    /// Get the rate limiter component
    pub fn rate_limiter(&self) -> &RateLimiter<S> {
        &self.rate_limiter
    }

    /// Get the quota manager component
    pub fn quota_manager(&self) -> &QuotaManager<S> {
        &self.quota_manager
    }

    /// Get the global settings
    pub fn settings(&self) -> &GlobalRateLimitSettings {
        self.rate_limiter.settings()
    }

    /// Update global settings
    pub fn update_settings(&mut self, settings: GlobalRateLimitSettings) {
        self.rate_limiter.update_settings(settings);
    }
}

impl<S> Clone for UnifiedLimiter<S>
where
    S: Store,
{
    fn clone(&self) -> Self {
        Self {
            rate_limiter: self.rate_limiter.clone(),
            quota_manager: self.quota_manager.clone(),
        }
    }
}

/// Combined status information
#[derive(Debug, Clone)]
pub struct LimiterStatus {
    /// Rate limiting status
    pub rate_limit: RateLimitDecision,
    /// Quota status (None if no quota configured)
    pub quota: Option<QuotaStatus>,
}

/// Result of reset operations
#[derive(Debug, Clone)]
pub struct ResetResult {
    /// Whether rate limit was reset
    pub rate_limit_reset: bool,
    /// Whether quota was reset
    pub quota_reset: bool,
}

impl ResetResult {
    /// Check if any reset operation succeeded
    pub fn any_reset(&self) -> bool {
        self.rate_limit_reset || self.quota_reset
    }

    /// Check if all reset operations succeeded
    pub fn all_reset(&self) -> bool {
        self.rate_limit_reset && self.quota_reset
    }
}

/// Builder for creating unified limiters
pub struct UnifiedLimiterBuilder<S>
where
    S: Store,
{
    store: Option<S>,
    settings: GlobalRateLimitSettings,
}

impl<S> UnifiedLimiterBuilder<S>
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

    /// Set the complete global settings
    pub fn settings(mut self, settings: GlobalRateLimitSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Build the unified limiter
    pub fn build(self) -> Result<UnifiedLimiter<S>>
    where
        S: Clone,
    {
        let store = self
            .store
            .ok_or_else(|| RateLimitError::internal("store is required"))?;

        self.settings
            .default_config
            .validate()
            .map_err(|msg| RateLimitError::invalid_config(&msg))?;

        Ok(UnifiedLimiter::from_store(store, self.settings))
    }
}

impl<S> Default for UnifiedLimiterBuilder<S>
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
    use crate::quota::QuotaConfig;
    use std::net::Ipv4Addr;
    use store::MemoryStore;

    #[tokio::test]
    async fn test_unified_limiter_basic_functionality() {
        let store = MemoryStore::new();
        let limiter = UnifiedLimiter::with_store(store);

        // First request should be allowed (no quota configured)
        let decision = limiter.check_request("test_key", 1, None).await.unwrap();
        assert!(decision.allowed);
        assert!(!decision.is_rate_limit_violation());
        assert!(!decision.is_quota_violation());
        assert!(decision.quota.is_none());
    }

    #[tokio::test]
    async fn test_unified_limiter_with_quota() {
        let store = MemoryStore::new();
        let limiter = UnifiedLimiter::with_store(store);

        // Set up config with quota and larger rate limit to accommodate test
        let config = RateLimitConfig::new(10, 100, 60) // Increase burst to 100
            .with_quota(QuotaConfig::daily_only(1000));
        limiter
            .set_config("test_key", config.clone(), None)
            .await
            .unwrap();

        // Request should be allowed (cost 50 is within burst of 100)
        let decision = limiter
            .check_request("test_key", 50, Some(config))
            .await
            .unwrap();

        assert!(decision.allowed);
        assert!(decision.quota.is_some());
        assert!(decision.quota.as_ref().unwrap().allowed);
    }

    #[tokio::test]
    async fn test_unified_limiter_rate_limit_violation() {
        let store = MemoryStore::new();
        let limiter = UnifiedLimiter::with_store(store);

        // Exhaust rate limit first
        for _ in 0..20 {
            limiter.check_request("test_key", 1, None).await.unwrap();
        }

        // Should be rate limited
        let decision = limiter.check_request("test_key", 1, None).await.unwrap();
        assert!(!decision.allowed);
        assert!(decision.is_rate_limit_violation());
        assert!(!decision.is_quota_violation());
        assert!(decision.retry_after_ms().is_some());
    }

    #[tokio::test]
    async fn test_unified_limiter_quota_violation() {
        let store = MemoryStore::new();
        let limiter = UnifiedLimiter::with_store(store);

        // Set up config with small quota
        let config = RateLimitConfig::new(1000, 2000, 60) // Very permissive rate limit
            .with_quota(QuotaConfig::daily_only(50));
        limiter
            .set_config("test_key", config.clone(), None)
            .await
            .unwrap();

        // Consume most quota
        let decision = limiter
            .check_and_consume("test_key", 45, Some(config.clone()))
            .await
            .unwrap();
        assert!(decision.allowed);

        // Should hit quota limit
        let decision = limiter
            .check_request("test_key", 10, Some(config))
            .await
            .unwrap();
        assert!(!decision.allowed);
        assert!(!decision.is_rate_limit_violation());
        assert!(decision.is_quota_violation());
        assert_eq!(
            decision.denial_reason,
            Some(DenialReason::Quota(ResetPeriod::Daily))
        );
    }

    #[tokio::test]
    async fn test_unified_limiter_check_and_consume() {
        let store = MemoryStore::new();
        let limiter = UnifiedLimiter::with_store(store);

        let config = RateLimitConfig::new(10, 50, 60) // Increase burst to 50
            .with_quota(QuotaConfig::daily_only(1000));
        limiter
            .set_config("test_key", config.clone(), None)
            .await
            .unwrap();

        // Check and consume should work (cost 25 is within burst of 50)
        let decision = limiter
            .check_and_consume("test_key", 25, Some(config))
            .await
            .unwrap();
        assert!(decision.allowed);

        // Status should show consumption
        let status = limiter.get_status("test_key", None).await.unwrap();
        assert!(status.quota.is_some());
        assert_eq!(
            status
                .quota
                .as_ref()
                .unwrap()
                .periods
                .get(&ResetPeriod::Daily)
                .unwrap()
                .used,
            25
        );
    }

    #[tokio::test]
    async fn test_unified_limiter_ip_functionality() {
        let store = MemoryStore::new();
        let limiter = UnifiedLimiter::with_store(store);

        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));

        // Set IP config with quota and larger burst
        let config = RateLimitConfig::new(10, 200, 60) // Increase burst to 200
            .with_quota(QuotaConfig::daily_only(5000));
        limiter.set_ip_config(ip, config, None).await.unwrap();

        // Should allow IP requests (cost 100 is within burst of 200)
        let decision = limiter.check_ip_request(ip, 100).await.unwrap();
        assert!(decision.allowed);

        // Check and consume (cost 50 is well within remaining capacity)
        let decision = limiter.check_and_consume_ip(ip, 50).await.unwrap();
        assert!(decision.allowed);

        // Check status
        let status = limiter.get_ip_status(ip).await.unwrap();
        assert!(status.quota.is_some());
    }

    #[tokio::test]
    async fn test_unified_limiter_reset_all() {
        let store = MemoryStore::new();
        let limiter = UnifiedLimiter::with_store(store);

        let config = RateLimitConfig::new(10, 100, 60) // Increase burst to 100
            .with_quota(QuotaConfig::daily_only(1000));
        limiter
            .set_config("test_key", config.clone(), None)
            .await
            .unwrap();

        // Use some resources (cost 50 is within burst of 100)
        limiter
            .check_and_consume("test_key", 50, Some(config))
            .await
            .unwrap();

        // Reset all
        let result = limiter.reset_all("test_key").await.unwrap();
        assert!(result.rate_limit_reset);
        assert!(result.quota_reset);
        assert!(result.any_reset());
        assert!(result.all_reset());

        // Status should be clean
        let status = limiter.get_status("test_key", None).await.unwrap();
        assert_eq!(status.rate_limit.remaining_tokens, 100.0); // Custom bucket size
        assert!(status.quota.is_none()); // Quota tracker removed
    }

    #[tokio::test]
    async fn test_unified_limiter_builder() {
        let store = MemoryStore::new();
        let custom_config = RateLimitConfig::new(100, 200, 60);

        let limiter = UnifiedLimiterBuilder::new()
            .store(store)
            .default_config(custom_config.clone())
            .enabled(true)
            .cleanup_interval(Duration::from_secs(600))
            .max_buckets(5000)
            .build()
            .unwrap();

        assert_eq!(limiter.settings().default_config, custom_config);
        assert!(limiter.settings().enabled);
    }

    #[tokio::test]
    async fn test_unified_decision_methods() {
        let rate_decision = RateLimitDecision::allowed(10.0, 20.0, RateLimitConfig::default());
        let quota_decision = QuotaDecision::allowed(
            crate::quota::QuotaStatus {
                periods: std::collections::HashMap::new(),
                config: QuotaConfig::default(),
            },
            QuotaConfig::default(),
        );

        let allowed_decision =
            UnifiedDecision::allowed(rate_decision.clone(), Some(quota_decision));
        assert!(allowed_decision.allowed);
        assert!(!allowed_decision.is_rate_limit_violation());
        assert!(!allowed_decision.is_quota_violation());

        let denied_decision = UnifiedDecision::denied(rate_decision, None, DenialReason::RateLimit);
        assert!(!denied_decision.allowed);
        assert!(denied_decision.is_rate_limit_violation());
        assert!(!denied_decision.is_quota_violation());
    }

    #[tokio::test]
    async fn test_unified_limiter_invalid_key() {
        let store = MemoryStore::new();
        let limiter = UnifiedLimiter::with_store(store);

        // Empty key should return error
        let result = limiter.check_request("", 1, None).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimitError::InvalidKey { .. }
        ));
    }

    #[tokio::test]
    async fn test_unified_limiter_cost_calculation() {
        let store = MemoryStore::new();
        let limiter = UnifiedLimiter::with_store(store);

        let config = RateLimitConfig::new(10, 20, 60) // Small rate limit for testing
            .with_quota(QuotaConfig::daily_only(1000));
        limiter
            .set_config("test_key", config.clone(), None)
            .await
            .unwrap();

        // High cost request should consume more tokens
        let decision = limiter
            .check_request("test_key", 15, Some(config))
            .await
            .unwrap();
        assert!(decision.allowed);
        assert_eq!(decision.rate_limit.remaining_tokens, 5.0); // 20 - 15 = 5

        // Next high cost request should be denied by rate limit
        let decision = limiter.check_request("test_key", 10, None).await.unwrap();
        assert!(!decision.allowed);
        assert!(decision.is_rate_limit_violation());
    }
}
