//! Rate limit result types
//!
//! This module contains the result types returned by rate limiting operations,
//! providing comprehensive information about the rate limiting decision.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Result of a rate limiting check
///
/// This structure contains all the information about whether a request
/// was allowed or denied, along with metadata about the current state
/// of the rate limiter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitDecision {
    /// Whether the request was allowed
    pub allowed: bool,

    /// The rate limit for this limiter
    pub limit: u64,

    /// Number of requests remaining in the current window
    pub remaining: u64,

    /// Unix timestamp when the rate limit will reset
    pub reset_time: u64,

    /// Duration to wait before retrying (if denied)
    pub retry_after: Option<Duration>,
}

impl RateLimitDecision {
    /// Creates a new result indicating the request was allowed
    pub fn allowed(limit: u64, remaining: u64, reset_time: u64) -> Self {
        Self {
            allowed: true,
            limit,
            remaining,
            reset_time,
            retry_after: None,
        }
    }

    /// Creates a new result indicating the request was denied
    pub fn denied(
        limit: u64,
        remaining: u64,
        reset_time: u64,
        retry_after: Option<Duration>,
    ) -> Self {
        Self {
            allowed: false,
            limit,
            remaining,
            reset_time,
            retry_after,
        }
    }

    /// Returns true if the request was allowed
    pub fn is_allowed(&self) -> bool {
        self.allowed
    }

    /// Returns true if the request was rate limited (denied)
    pub fn is_rate_limited(&self) -> bool {
        !self.allowed
    }

    /// Returns true if the rate limiter is exhausted (no remaining capacity)
    pub fn is_exhausted(&self) -> bool {
        self.remaining == 0
    }

    /// Returns the retry after duration in seconds, if available
    pub fn retry_after_secs(&self) -> Option<u64> {
        self.retry_after.map(|d| d.as_secs())
    }

    /// Returns the retry after duration in milliseconds, if available
    pub fn retry_after_millis(&self) -> Option<u64> {
        self.retry_after.map(|d| d.as_millis() as u64)
    }

    /// Returns the time until reset in seconds from now
    pub fn time_until_reset(&self, now: u64) -> u64 {
        self.reset_time.saturating_sub(now)
    }

    /// Returns true if the rate limit has already reset
    pub fn has_reset(&self, now: u64) -> bool {
        now >= self.reset_time
    }

    /// Merges multiple rate limit results into a single result
    ///
    /// The merged result will be allowed only if ALL input results are allowed.
    /// The most restrictive limits will be used for the merged result.
    pub fn merge(results: Vec<RateLimitDecision>) -> Option<Self> {
        if results.is_empty() {
            return None;
        }

        let all_allowed = results.iter().all(|r| r.allowed);

        // Find the most restrictive result (lowest remaining count)
        let most_restrictive = results.iter().min_by_key(|r| r.remaining).unwrap();

        // Find the earliest reset time
        let earliest_reset = results.iter().map(|r| r.reset_time).min().unwrap();

        // Find the shortest retry after time (if any)
        let shortest_retry = results.iter().filter_map(|r| r.retry_after).min();

        Some(Self {
            allowed: all_allowed,
            limit: most_restrictive.limit,
            remaining: most_restrictive.remaining,
            reset_time: earliest_reset,
            retry_after: if all_allowed { None } else { shortest_retry },
        })
    }
}

/// Information about rate limiting for API responses
///
/// This is a simplified version of RateLimitResult suitable for
/// including in API response headers or JSON responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    /// The rate limit for this endpoint/user
    pub limit: u64,

    /// Number of requests remaining
    pub remaining: u64,

    /// Unix timestamp when the limit resets
    pub reset_time: u64,

    /// Milliseconds to wait before retrying (if rate limited)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
}

impl From<RateLimitDecision> for RateLimitInfo {
    fn from(result: RateLimitDecision) -> Self {
        Self {
            limit: result.limit,
            remaining: result.remaining,
            reset_time: result.reset_time,
            retry_after_ms: result.retry_after.map(|d| d.as_millis() as u64),
        }
    }
}

impl RateLimitInfo {
    /// Creates rate limit info from individual components
    pub fn new(limit: u64, remaining: u64, reset_time: u64) -> Self {
        Self {
            limit,
            remaining,
            reset_time,
            retry_after_ms: None,
        }
    }

    /// Sets the retry after duration
    pub fn with_retry_after(mut self, retry_after: Duration) -> Self {
        self.retry_after_ms = Some(retry_after.as_millis() as u64);
        self
    }

    /// Returns true if there are no remaining requests
    pub fn is_exhausted(&self) -> bool {
        self.remaining == 0
    }

    /// Returns the retry after duration, if set
    pub fn retry_after(&self) -> Option<Duration> {
        self.retry_after_ms.map(Duration::from_millis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_result_creation() {
        let allowed = RateLimitDecision::allowed(100, 50, 1234567890);
        assert!(allowed.is_allowed());
        assert!(!allowed.is_rate_limited());
        assert!(!allowed.is_exhausted());
        assert_eq!(allowed.limit, 100);
        assert_eq!(allowed.remaining, 50);
        assert_eq!(allowed.reset_time, 1234567890);
        assert!(allowed.retry_after.is_none());

        let denied = RateLimitDecision::denied(100, 0, 1234567890, Some(Duration::from_secs(30)));
        assert!(!denied.is_allowed());
        assert!(denied.is_rate_limited());
        assert!(denied.is_exhausted());
        assert_eq!(denied.retry_after_secs(), Some(30));
        assert_eq!(denied.retry_after_millis(), Some(30000));
    }

    #[test]
    fn test_time_calculations() {
        let result = RateLimitDecision::allowed(100, 50, 1234567890);

        assert_eq!(result.time_until_reset(1234567880), 10);
        assert_eq!(result.time_until_reset(1234567890), 0);
        assert_eq!(result.time_until_reset(1234567900), 0); // Saturating sub

        assert!(!result.has_reset(1234567880));
        assert!(result.has_reset(1234567890));
        assert!(result.has_reset(1234567900));
    }

    #[test]
    fn test_merge_results() {
        let result1 = RateLimitDecision::allowed(100, 50, 1234567890);
        let result2 = RateLimitDecision::allowed(200, 75, 1234567900);
        let result3 = RateLimitDecision::denied(50, 0, 1234567880, Some(Duration::from_secs(10)));

        // Merge all allowed results
        let merged_allowed =
            RateLimitDecision::merge(vec![result1.clone(), result2.clone()]).unwrap();
        assert!(merged_allowed.is_allowed());
        assert_eq!(merged_allowed.remaining, 50); // Most restrictive
        assert_eq!(merged_allowed.reset_time, 1234567890); // Earliest reset

        // Merge with one denied result
        let merged_denied = RateLimitDecision::merge(vec![result1, result2, result3]).unwrap();
        assert!(merged_denied.is_rate_limited());
        assert_eq!(merged_denied.remaining, 0); // Most restrictive
        assert_eq!(merged_denied.retry_after, Some(Duration::from_secs(10)));

        // Empty merge
        assert!(RateLimitDecision::merge(vec![]).is_none());
    }

    #[test]
    fn test_rate_limit_info() {
        let result =
            RateLimitDecision::denied(100, 0, 1234567890, Some(Duration::from_millis(5500)));

        let info: RateLimitInfo = result.into();
        assert_eq!(info.limit, 100);
        assert_eq!(info.remaining, 0);
        assert_eq!(info.reset_time, 1234567890);
        assert_eq!(info.retry_after_ms, Some(5500));
        assert!(info.is_exhausted());
        assert_eq!(info.retry_after(), Some(Duration::from_millis(5500)));

        let info2 =
            RateLimitInfo::new(200, 50, 1234567890).with_retry_after(Duration::from_secs(30));
        assert_eq!(info2.retry_after_ms, Some(30000));
    }

    // #[test]
    // fn test_serialization() {
    //     let result = RateLimitResult::allowed(100, 50, 1234567890);
    //     let json = serde_json::to_string(&result).unwrap();
    //     let deserialized: RateLimitResult = serde_json::from_str(&json).unwrap();

    //     assert_eq!(result.allowed, deserialized.allowed);
    //     assert_eq!(result.limit, deserialized.limit);
    //     assert_eq!(result.remaining, deserialized.remaining);
    //     assert_eq!(result.reset_time, deserialized.reset_time);
    // }
}
