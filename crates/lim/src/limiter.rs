use crate::decision::RateLimitDecision;
use crate::error::{RateLimitError, Result};
use crate::strategies::{LimitKind, RateLimit};
use std::collections::HashMap;

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::Limiter;
    use crate::clock::CachedClock;
    use crate::state::State;
    use crate::strategies::gcra::{Gcra, Quota};
    use core::num::NonZeroU32;
    use core::time::Duration;
    use std::sync::Arc;

    fn gcra(state: &Arc<State>, clock: &Arc<CachedClock>, burst: u32, replenish: Duration) -> Gcra {
        Gcra::new(
            state.clone(),
            clock.clone(),
            Quota {
                max_burst: NonZeroU32::new(burst).unwrap(),
                replenish_1_per: replenish,
            },
        )
    }

    /// Two named limits checking the same key must not share replenish
    /// state: exhausting a strict limit must not throttle a permissive one
    /// (e.g. an org whose attrs switch it to a different named limit).
    #[tokio::test]
    async fn named_limits_do_not_share_state_for_the_same_key() {
        let state = Arc::new(State::new());
        let clock = CachedClock::new();
        let mut limiter = Limiter::new();
        limiter.add_limit(
            "strict".to_string(),
            Box::new(gcra(&state, &clock, 2, Duration::from_secs(600))),
        );
        limiter.add_limit(
            "permissive".to_string(),
            Box::new(gcra(&state, &clock, 100, Duration::from_secs(1))),
        );

        let key = "lim:org:01ORG";

        // Exhaust the strict limit.
        assert!(
            limiter
                .check("strict", key, None)
                .await
                .unwrap()
                .is_allowed()
        );
        assert!(
            limiter
                .check("strict", key, None)
                .await
                .unwrap()
                .is_allowed()
        );
        assert!(
            !limiter
                .check("strict", key, None)
                .await
                .unwrap()
                .is_allowed()
        );

        // The permissive limit for the same key starts fresh.
        let decision = limiter.check("permissive", key, None).await.unwrap();
        assert!(
            decision.is_allowed(),
            "permissive limit must not inherit the strict limit's state"
        );
    }
}

pub struct Limiter {
    limits: HashMap<String, Box<dyn RateLimit>>,
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
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

    pub fn get_limit(&self, name: &str) -> Option<&dyn RateLimit> {
        self.limits.get(name).map(Box::as_ref)
    }

    pub fn limit_names(&self) -> Vec<String> {
        self.limits.keys().cloned().collect()
    }

    pub fn has_limit(&self, name: &str) -> bool {
        self.limits.contains_key(name)
    }

    pub fn validate_limit(&self, name: &str, expected: LimitKind) -> Result<()> {
        let limit = self
            .get_limit(name)
            .ok_or_else(|| RateLimitError::UnknownLimit(name.into()))?;
        let actual = limit.kind();
        if actual != expected {
            return Err(RateLimitError::IncompatibleLimit {
                name: name.into(),
                expected,
                actual,
            });
        }
        Ok(())
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
        let limiter = self
            .get_limit(limiter_name)
            .ok_or_else(|| RateLimitError::UnknownLimit(limiter_name.into()))?;
        let cost = cost.unwrap_or(1);
        // Namespace the stored state by limit name: two named limits
        // checking the same key (e.g. a subject or org whose attrs
        // switch them to a different named limit) must not share
        // replenish state — a strict limit's TAT would keep throttling
        // long after the switch to a permissive one.
        let state_key = format!("{limiter_name}:{key}");
        limiter.check(&state_key, cost).await
    }
}

#[cfg(test)]
mod configuration_tests {
    use super::*;

    #[tokio::test]
    async fn unknown_selected_names_are_typed_errors_even_without_any_limits() {
        let limiter = Limiter::new();
        for name in ["default", "typo", ""] {
            assert!(matches!(limiter.check(name, "valid-key", None).await,
                Err(RateLimitError::UnknownLimit(missing)) if missing == name));
            assert!(matches!(limiter.validate_limit(name, LimitKind::Rate),
                Err(RateLimitError::UnknownLimit(missing)) if missing == name));
        }
    }
}
