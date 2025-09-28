use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::quota::ResetPeriod;

/// Rate limiting configuration for a specific key or default configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RateLimitConfig {
    /// Maximum number of requests allowed per second
    pub requests_per_second: u32,

    /// Maximum burst size - number of requests that can be made in a short burst
    pub burst_size: u32,

    /// Time window in seconds for rate limiting calculations
    pub window_size_seconds: u64,

    /// Quotas for different reset periods.
    pub quotas: Option<std::collections::HashMap<ResetPeriod, u64>>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 10,
            burst_size: 20,
            window_size_seconds: 60,
            quotas: None,
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
            quotas: None,
        }
    }

    pub fn with_quotas(mut self, quotas: std::collections::HashMap<ResetPeriod, u64>) -> Self {
        self.quotas = Some(quotas);
        self
    }

    /// Create a permissive rate limit configuration (high limits)
    pub fn permissive() -> Self {
        Self {
            requests_per_second: 1000,
            burst_size: 2000,
            window_size_seconds: 60,
            quotas: None,
        }
    }

    /// Create a restrictive rate limit configuration (low limits)
    pub fn restrictive() -> Self {
        Self {
            requests_per_second: 1,
            burst_size: 5,
            window_size_seconds: 60,
            quotas: None,
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

        if let Some(quotas) = &self.quotas {
            for (&period, &limit) in quotas {
                if limit == 0 {
                    return Err(format!(
                        "Quota limit for {:?} must be greater than 0",
                        period
                    ));
                }
            }
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
}
