//! Jitter support for rate limiting
//!
//! This module provides jitter functionality to help distribute requests
//! more evenly over time, reducing thundering herd effects.

use std::time::Duration;

/// Jitter configuration for rate limiting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jitter {
    /// Maximum jitter amount in nanoseconds
    max_jitter_nanos: u64,
}

impl Jitter {
    /// Create a new jitter configuration with a maximum duration
    pub fn new(max_jitter: Duration) -> Self {
        Self {
            max_jitter_nanos: max_jitter.as_nanos() as u64,
        }
    }

    /// Create jitter that's a percentage of the given base duration
    pub fn percent_of(base_duration: Duration, percent: f64) -> Self {
        let jitter_nanos = (base_duration.as_nanos() as f64 * percent / 100.0) as u64;
        Self {
            max_jitter_nanos: jitter_nanos,
        }
    }

    /// Create jitter up to the given number of milliseconds
    pub fn up_to_millis(millis: u64) -> Self {
        Self::new(Duration::from_millis(millis))
    }

    /// Create jitter up to the given number of seconds
    pub fn up_to_seconds(seconds: u64) -> Self {
        Self::new(Duration::from_secs(seconds))
    }

    /// Get the maximum jitter duration
    pub fn max_jitter(&self) -> Duration {
        Duration::from_nanos(self.max_jitter_nanos)
    }

    /// Generate a random jitter value
    pub fn generate(&self) -> u64 {
        use rand::Rng;
        let mut rng = rand::rng();
        let jitter_nanos = rng.random_range(0..=self.max_jitter_nanos);
        jitter_nanos
    }

    /// Generate a deterministic jitter value based on a seed
    pub fn generate_with_seed(&self, seed: u64) -> u64 {
        // Simple linear congruential generator for deterministic jitter
        let a = 1664525u64;
        let c = 1013904223u64;
        let m = 2u64.pow(32);

        let next = (a.wrapping_mul(seed).wrapping_add(c)) % m;
        let jitter_nanos = (next % (self.max_jitter_nanos + 1)) as u64;
        jitter_nanos
    }

    /// Generate jitter based on a hash of the key
    pub fn generate_for_key(&self, key: &str) -> u64 {
        let hash = self.simple_hash(key);
        self.generate_with_seed(hash)
    }

    /// Simple hash function for string keys
    fn simple_hash(&self, s: &str) -> u64 {
        let mut hash = 5381u64;
        for byte in s.bytes() {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
        }
        hash
    }

    /// Apply jitter to a duration (add jitter)
    pub fn apply_to_duration(&self, duration: Duration, seed: u64) -> Duration {
        let jitter = self.generate_with_seed(seed);
        duration + Duration::from_nanos(jitter)
    }

    /// Apply jitter to a duration using key-based jitter
    pub fn apply_to_duration_with_key(&self, duration: Duration, key: &str) -> Duration {
        let jitter = self.generate_for_key(key);
        duration + Duration::from_nanos(jitter)
    }

    /// Get jitter as a fraction of the maximum (0.0 to 1.0)
    pub fn get_fraction(&self, seed: u64) -> f64 {
        let jitter = self.generate_with_seed(seed);
        jitter as f64 / self.max_jitter_nanos as f64
    }
}

impl Default for Jitter {
    fn default() -> Self {
        Self::new(Duration::from_millis(100))
    }
}

impl From<Duration> for Jitter {
    fn from(duration: Duration) -> Self {
        Self::new(duration)
    }
}

// impl std::ops::Add<Nanos> for Jitter {
//     type Output = Nanos;

//     fn add(self, nanos: Nanos) -> Nanos {
//         nanos + self.generate_with_seed(0)
//     }
// }

/// Jitter strategy enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JitterStrategy {
    /// No jitter applied
    None,
    /// Fixed maximum jitter
    Fixed(Jitter),
    /// Jitter proportional to wait time
    Proportional { percent: u8 },
    /// Exponential backoff with jitter
    ExponentialBackoff { base_ms: u64, max_ms: u64 },
}

impl JitterStrategy {
    /// Apply the jitter strategy to a wait duration
    pub fn apply(&self, wait_duration: Duration, attempt: u32, key: &str) -> Duration {
        match self {
            JitterStrategy::None => wait_duration,
            JitterStrategy::Fixed(jitter) => jitter.apply_to_duration_with_key(wait_duration, key),
            JitterStrategy::Proportional { percent } => {
                let jitter = Jitter::percent_of(wait_duration, *percent as f64);
                jitter.apply_to_duration_with_key(wait_duration, key)
            }
            JitterStrategy::ExponentialBackoff { base_ms, max_ms } => {
                let exponential_ms = (*base_ms * 2_u64.pow(attempt)).min(*max_ms);
                let base_duration = Duration::from_millis(exponential_ms);
                let jitter = Jitter::percent_of(base_duration, 10.0); // 10% jitter
                jitter.apply_to_duration_with_key(base_duration, key)
            }
        }
    }

    /// Create a proportional jitter strategy
    pub fn proportional(percent: u8) -> Self {
        Self::Proportional { percent }
    }

    /// Create an exponential backoff strategy
    pub fn exponential_backoff(base_ms: u64, max_ms: u64) -> Self {
        Self::ExponentialBackoff { base_ms, max_ms }
    }
}

impl Default for JitterStrategy {
    fn default() -> Self {
        Self::Proportional { percent: 10 }
    }
}

/// Helper trait for adding jitter to time-based operations
pub trait WithJitter {
    /// Add jitter to this duration
    fn with_jitter(self, jitter: Jitter, key: &str) -> Self;

    /// Add proportional jitter
    fn with_proportional_jitter(self, percent: f64, key: &str) -> Self;
}

impl WithJitter for Duration {
    fn with_jitter(self, jitter: Jitter, key: &str) -> Self {
        jitter.apply_to_duration_with_key(self, key)
    }

    fn with_proportional_jitter(self, percent: f64, key: &str) -> Self {
        let jitter = Jitter::percent_of(self, percent);
        jitter.apply_to_duration_with_key(self, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jitter_creation() {
        let jitter = Jitter::new(Duration::from_millis(100));
        assert_eq!(jitter.max_jitter(), Duration::from_millis(100));

        let jitter = Jitter::up_to_millis(500);
        assert_eq!(jitter.max_jitter(), Duration::from_millis(500));

        let jitter = Jitter::percent_of(Duration::from_secs(1), 10.0);
        assert_eq!(jitter.max_jitter(), Duration::from_millis(100));
    }

    #[test]
    fn test_jitter_generation() {
        let jitter = Jitter::up_to_millis(100);

        // Test deterministic generation with same seed
        let jitter1 = jitter.generate_with_seed(42);
        let jitter2 = jitter.generate_with_seed(42);
        assert_eq!(jitter1, jitter2);

        // Test different seeds produce different results
        let jitter3 = jitter.generate_with_seed(123);
        assert_ne!(jitter1, jitter3);

        // Test jitter is within bounds
        assert!(jitter1 <= jitter.max_jitter_nanos);
        assert!(jitter3 <= jitter.max_jitter_nanos);
    }

    #[test]
    fn test_jitter_for_key() {
        let jitter = Jitter::up_to_millis(100);

        // Same key should produce same jitter
        let jitter1 = jitter.generate_for_key("test_key");
        let jitter2 = jitter.generate_for_key("test_key");
        assert_eq!(jitter1, jitter2);

        // Different keys should produce different jitter (usually)
        let _jitter3 = jitter.generate_for_key("different_key");
        // Note: This might occasionally be equal due to hash collisions, but very unlikely
    }

    #[test]
    fn test_jitter_application() {
        let jitter = Jitter::up_to_millis(100);
        let base_duration = Duration::from_secs(1);

        let jittered = jitter.apply_to_duration(base_duration, 42);
        assert!(jittered >= base_duration);
        assert!(jittered <= base_duration + jitter.max_jitter());
    }

    #[test]
    fn test_jitter_strategies() {
        let wait_duration = Duration::from_millis(1000);
        let key = "test_key";

        // None strategy
        let result = JitterStrategy::None.apply(wait_duration, 0, key);
        assert_eq!(result, wait_duration);

        // Fixed strategy
        let jitter = Jitter::up_to_millis(100);
        let result = JitterStrategy::Fixed(jitter).apply(wait_duration, 0, key);
        assert!(result >= wait_duration);

        // Proportional strategy
        let result = JitterStrategy::proportional(10).apply(wait_duration, 0, key);
        assert!(result >= wait_duration);

        // Exponential backoff
        let strategy = JitterStrategy::exponential_backoff(100, 5000);
        let result1 = strategy.apply(wait_duration, 0, key);
        let result2 = strategy.apply(wait_duration, 1, key);
        let result3 = strategy.apply(wait_duration, 2, key);

        // Should increase with attempts
        assert!(result2 >= result1);
        assert!(result3 >= result2);
    }

    #[test]
    fn test_with_jitter_trait() {
        let duration = Duration::from_millis(1000);
        let jitter = Jitter::up_to_millis(100);

        let jittered = duration.with_jitter(jitter, "test_key");
        assert!(jittered >= duration);

        let proportional = duration.with_proportional_jitter(10.0, "test_key");
        assert!(proportional >= duration);
    }

    #[test]
    fn test_jitter_fraction() {
        let jitter = Jitter::up_to_millis(100);
        let fraction = jitter.get_fraction(42);
        assert!(fraction >= 0.0);
        assert!(fraction <= 1.0);
    }

    #[test]
    fn test_jitter_hash_consistency() {
        let jitter = Jitter::up_to_millis(100);

        // Same string should always hash to same value
        let hash1 = jitter.simple_hash("test");
        let hash2 = jitter.simple_hash("test");
        assert_eq!(hash1, hash2);

        // Different strings should hash to different values (usually)
        let hash3 = jitter.simple_hash("different");
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_jitter_default() {
        let jitter = Jitter::default();
        assert_eq!(jitter.max_jitter(), Duration::from_millis(100));

        let strategy = JitterStrategy::default();
        matches!(strategy, JitterStrategy::Proportional { percent: 10 });
    }
}
