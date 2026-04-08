use crate::decision::RateLimitDecision;
use crate::error::{RateLimitError, Result};
use crate::strategies::RateLimit;
use std::collections::HashMap;

pub struct Limiter {
    limits: HashMap<String, Box<dyn RateLimit>>,
}

impl Limiter {
    pub fn new() -> Self {
        Self {
            limits: HashMap::new(),
        }
    }

    pub fn add_limit(&mut self, name: String, limit: Box<dyn RateLimit>) {
        self.limits.insert(name, limit);
    }

    pub fn get_limit(&self, name: &str) -> Option<&Box<dyn RateLimit>> {
        self.limits.get(name)
    }

    pub fn limit_names(&self) -> Vec<String> {
        self.limits.keys().cloned().collect()
    }

    pub fn has_limit(&self, name: &str) -> bool {
        self.limits.contains_key(name)
    }

    pub async fn check(
        &self,
        limiter_name: &str,
        key: &str,
        cost: Option<u64>,
    ) -> Result<RateLimitDecision> {
        if key.is_empty() || key.len() > 256 {
            return Err(RateLimitError::InvalidConfig("Invalid key".to_string()));
        }
        if let Some(limiter) = self.limits.get(limiter_name) {
            let cost = cost.unwrap_or(1);
            limiter.check(key, cost).await
        } else {
            // No limit configured for this name — allow the request through.
            // This prevents 500 errors on clean boot when limits config is null.
            Ok(RateLimitDecision::allowed(0, 0, None))
        }
    }
}
