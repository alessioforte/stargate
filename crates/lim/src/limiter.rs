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

    // TODO: Add optional cost
    pub async fn check(&self, limiter_name: &str, key: &str) -> Result<RateLimitDecision> {
        if key.is_empty() || key.len() > 256 {
            return Err(RateLimitError::InvalidConfig("Invalid key".to_string()));
        }
        if let Some(limiter) = self.limits.get(limiter_name) {
            limiter.check(key).await
        } else {
            Err(RateLimitError::InvalidConfig(limiter_name.to_string()))
        }
    }

    // pub async fn incr_count(&self, key: &str) -> u64 {
    //     // Increment the count in the store and return the new value
    //     let k = format!("rate_limit:{}", key);
    //     match self.store.incr_i64(&k, 1, None).await {
    //         Ok(Some(count)) => count as u64,
    //         _ => 0,
    //     }
    // }
}
