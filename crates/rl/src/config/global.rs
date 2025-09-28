use super::limit::RateLimitConfig;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Global rate limiting settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalRateLimitSettings {
    /// Default configuration for new keys
    pub default_config: RateLimitConfig,

    /// Whether to enable rate limiting globally
    pub enabled: bool,

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
            enabled: true,
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
        // default_ip_config: RateLimitConfig,
    ) -> Self {
        Self {
            default_config,
            // default_ip_config,
            ..Default::default()
        }
    }

    /// Create settings optimized for API gateway usage with IP-based rate limiting
    pub fn for_api_gateway() -> Self {
        Self {
            default_config: RateLimitConfig::permissive(),
            enabled: true,
            cleanup_interval_seconds: 120, // More frequent cleanup for high-traffic
            max_buckets: 50000,            // Higher capacity
            config_cache_ttl_seconds: 600, // Longer cache for performance
        }
    }

    /// Create settings for development environments with relaxed IP limits
    pub fn for_development() -> Self {
        Self {
            default_config: RateLimitConfig::permissive(),
            // default_ip_config: RateLimitConfig::permissive_ip(),
            enabled: true,
            // ip_rate_limiting_enabled: false, // Disable IP limiting in dev
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
