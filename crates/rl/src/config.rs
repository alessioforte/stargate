use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::quota::QuotaConfig;

/// Rate limiting configuration for a specific key or default configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RateLimitConfig {
    /// Maximum number of requests allowed per second
    pub requests_per_second: u32,

    /// Maximum burst size - number of requests that can be made in a short burst
    pub burst_size: u32,

    /// Time window in seconds for rate limiting calculations
    pub window_size_seconds: u64,

    /// Quota configuration for daily and monthly limits
    pub quota: Option<QuotaConfig>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 10,
            burst_size: 20,
            window_size_seconds: 60,
            quota: None,
        }
    }
}

/// Default IP-based rate limiting configuration
/// More restrictive than default to prevent abuse from single IPs
impl RateLimitConfig {
    /// Default configuration for IP-based rate limiting
    /// More conservative than general rate limiting to prevent IP-based abuse
    pub fn default_ip() -> Self {
        Self {
            requests_per_second: 10, // 10 requests per second
            burst_size: 20,          // Burst allowance
            window_size_seconds: 60, // 1 minute window
            quota: None,
        }
    }
}

impl RateLimitConfig {
    /// Create a new rate limit configuration
    pub fn new(requests_per_second: u32, burst_size: u32, window_size_seconds: u64) -> Self {
        Self {
            requests_per_second,
            burst_size,
            window_size_seconds,
            quota: None,
        }
    }

    /// Create a new rate limit configuration with quota
    pub fn new_with_quota(
        requests_per_second: u32,
        burst_size: u32,
        window_size_seconds: u64,
        quota: QuotaConfig,
    ) -> Self {
        Self {
            requests_per_second,
            burst_size,
            window_size_seconds,
            quota: Some(quota),
        }
    }

    /// Create a permissive rate limit configuration (high limits)
    pub fn permissive() -> Self {
        Self {
            requests_per_second: 1000,
            burst_size: 2000,
            window_size_seconds: 60,
            quota: None,
        }
    }

    /// Create a restrictive rate limit configuration (low limits)
    pub fn restrictive() -> Self {
        Self {
            requests_per_second: 1,
            burst_size: 5,
            window_size_seconds: 60,
            quota: None,
        }
    }

    /// Create a restrictive IP-based rate limit configuration
    /// Very low limits for preventing abuse from individual IPs
    pub fn restrictive_ip() -> Self {
        Self {
            requests_per_second: 2, // 2 requests per second
            burst_size: 5,          // Small burst
            window_size_seconds: 60,
            quota: None,
        }
    }

    /// Create a permissive IP-based rate limit configuration
    /// Higher limits for trusted IP ranges or development environments
    pub fn permissive_ip() -> Self {
        Self {
            requests_per_second: 300, // 5 requests per second
            burst_size: 600,          // Generous burst allowance
            window_size_seconds: 60,
            quota: None,
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.requests_per_second == 0 {
            return Err("requests_per_second must be greater than 0".to_string());
        }

        if self.burst_size == 0 {
            return Err("burst_size must be greater than 0".to_string());
        }

        if self.burst_size < self.requests_per_second {
            return Err(
                "burst_size should be greater than or equal to requests_per_second".to_string(),
            );
        }

        if self.window_size_seconds == 0 {
            return Err("window_size_seconds must be greater than 0".to_string());
        }

        Ok(())
    }

    /// Get the refill rate in tokens per millisecond
    pub fn refill_rate_per_ms(&self) -> f64 {
        self.requests_per_second as f64 / 1000.0
    }

    /// Get the maximum tokens (burst size) as f64
    pub fn max_tokens(&self) -> f64 {
        self.burst_size as f64
    }

    /// Get the window size as Duration
    pub fn window_duration(&self) -> Duration {
        Duration::from_secs(self.window_size_seconds)
    }

    /// Check if quota is configured
    pub fn has_quota(&self) -> bool {
        self.quota.as_ref().map_or(false, |q| q.has_quotas())
    }

    /// Get quota configuration
    pub fn quota(&self) -> Option<&QuotaConfig> {
        self.quota.as_ref()
    }

    /// Set quota configuration
    pub fn with_quota(mut self, quota: QuotaConfig) -> Self {
        self.quota = Some(quota);
        self
    }

    /// Remove quota configuration
    pub fn without_quota(mut self) -> Self {
        self.quota = None;
        self
    }
}

/// Global rate limiting settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalRateLimitSettings {
    /// Default configuration for new keys
    pub default_config: RateLimitConfig,

    /// Default configuration for IP-based rate limiting
    /// Used when check_rate_limit_ip() is called without custom config
    pub default_ip_config: RateLimitConfig,

    /// Whether to enable rate limiting globally
    pub enabled: bool,

    /// Whether to enable IP-based rate limiting by default
    pub ip_rate_limiting_enabled: bool,

    /// Cleanup interval for expired buckets in seconds
    pub cleanup_interval_seconds: u64,

    /// Maximum number of rate limit buckets to keep in memory
    pub max_buckets: usize,

    /// TTL for cached configurations in seconds
    pub config_cache_ttl_seconds: u64,
}

impl Default for GlobalRateLimitSettings {
    fn default() -> Self {
        Self {
            default_config: RateLimitConfig::default(),
            default_ip_config: RateLimitConfig::default_ip(),
            enabled: true,
            ip_rate_limiting_enabled: true,
            cleanup_interval_seconds: 300, // 5 minutes
            max_buckets: 10000,
            config_cache_ttl_seconds: 300, // 5 minutes
        }
    }
}

impl GlobalRateLimitSettings {
    /// Create new global settings with default configuration
    pub fn new(default_config: RateLimitConfig) -> Self {
        Self {
            default_config,
            ..Default::default()
        }
    }

    /// Create new global settings with both default and IP configurations
    pub fn new_with_ip_config(
        default_config: RateLimitConfig,
        default_ip_config: RateLimitConfig,
    ) -> Self {
        Self {
            default_config,
            default_ip_config,
            ..Default::default()
        }
    }

    /// Create settings optimized for API gateway usage with IP-based rate limiting
    pub fn for_api_gateway() -> Self {
        Self {
            default_config: RateLimitConfig::permissive(),
            default_ip_config: RateLimitConfig::default_ip(),
            enabled: true,
            ip_rate_limiting_enabled: true,
            cleanup_interval_seconds: 120, // More frequent cleanup for high-traffic
            max_buckets: 50000,            // Higher capacity
            config_cache_ttl_seconds: 600, // Longer cache for performance
        }
    }

    /// Create settings for development environments with relaxed IP limits
    pub fn for_development() -> Self {
        Self {
            default_config: RateLimitConfig::permissive(),
            default_ip_config: RateLimitConfig::permissive_ip(),
            enabled: true,
            ip_rate_limiting_enabled: false, // Disable IP limiting in dev
            cleanup_interval_seconds: 600,
            max_buckets: 1000,
            config_cache_ttl_seconds: 300,
        }
    }

    /// Get cleanup interval as Duration
    pub fn cleanup_interval(&self) -> Duration {
        Duration::from_secs(self.cleanup_interval_seconds)
    }

    /// Get config cache TTL as Duration
    pub fn config_cache_ttl(&self) -> Duration {
        Duration::from_secs(self.config_cache_ttl_seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_config_validation() {
        let valid_config = RateLimitConfig::new(10, 20, 60);
        assert!(valid_config.validate().is_ok());

        let invalid_rps = RateLimitConfig::new(0, 20, 60);
        assert!(invalid_rps.validate().is_err());

        let invalid_burst = RateLimitConfig::new(10, 0, 60);
        assert!(invalid_burst.validate().is_err());

        let invalid_burst_size = RateLimitConfig::new(20, 10, 60);
        assert!(invalid_burst_size.validate().is_err());

        let invalid_window = RateLimitConfig::new(10, 20, 0);
        assert!(invalid_window.validate().is_err());
    }

    #[test]
    fn test_refill_rate_calculation() {
        let config = RateLimitConfig::new(1000, 2000, 60);
        assert_eq!(config.refill_rate_per_ms(), 1.0);

        let config = RateLimitConfig::new(100, 200, 60);
        assert_eq!(config.refill_rate_per_ms(), 0.1);
    }

    #[test]
    fn test_preset_configurations() {
        let permissive = RateLimitConfig::permissive();
        assert!(permissive.validate().is_ok());
        assert!(permissive.requests_per_second >= 1000);

        let restrictive = RateLimitConfig::restrictive();
        assert!(restrictive.validate().is_ok());
        assert!(restrictive.requests_per_second <= 5);
    }

    #[test]
    fn test_ip_preset_configurations() {
        let default_ip = RateLimitConfig::default_ip();
        assert!(default_ip.validate().is_ok());
        assert!(default_ip.requests_per_second >= 5);
        assert!(default_ip.requests_per_second <= 20);

        let restrictive_ip = RateLimitConfig::restrictive_ip();
        assert!(restrictive_ip.validate().is_ok());
        assert!(restrictive_ip.requests_per_second <= 5);

        let permissive_ip = RateLimitConfig::permissive_ip();
        assert!(permissive_ip.validate().is_ok());
        assert!(permissive_ip.requests_per_second >= 100);
    }

    #[test]
    fn test_global_settings_presets() {
        let api_gateway = GlobalRateLimitSettings::for_api_gateway();
        assert!(api_gateway.enabled);
        assert!(api_gateway.ip_rate_limiting_enabled);
        assert!(api_gateway.max_buckets >= 10000);

        let dev_settings = GlobalRateLimitSettings::for_development();
        assert!(dev_settings.enabled);
        assert!(!dev_settings.ip_rate_limiting_enabled);
    }
}
