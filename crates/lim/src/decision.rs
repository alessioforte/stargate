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

    /// The capacity limit for this rate limiter
    pub limit: u64,

    /// Number of requests remaining in the current window
    pub remaining: u64,

    /// Duration to wait before retrying (if denied).
    /// All strategies normalize this to millisecond resolution.
    pub retry_after: Option<Duration>,

    /// Duration until the rate limit resets
    pub reset: Option<Duration>,
}

impl RateLimitDecision {
    /// Creates a new result indicating the request was allowed
    pub fn allowed(limit: u64, remaining: u64, reset: Option<Duration>) -> Self {
        Self {
            allowed: true,
            limit,
            remaining,
            reset,
            retry_after: None,
        }
    }

    /// Creates a new result indicating the request was denied
    pub fn denied(
        limit: u64,
        remaining: u64,
        retry_after: Option<Duration>,
        reset: Option<Duration>,
    ) -> Self {
        Self {
            allowed: false,
            limit,
            remaining,
            retry_after,
            reset,
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

    /// Returns the retry after duration in whole seconds (ceiling), if available.
    /// Use this for HTTP `Retry-After` header values.
    pub fn retry_after_secs(&self) -> Option<u64> {
        self.retry_after.map(|d| {
            let secs = d.as_secs();
            if d.subsec_millis() > 0 {
                secs + 1
            } else {
                secs
            }
        })
    }

    /// Returns the retry after duration in milliseconds, if available
    pub fn retry_after_millis(&self) -> Option<u64> {
        self.retry_after.map(|d| d.as_millis() as u64)
    }
}
