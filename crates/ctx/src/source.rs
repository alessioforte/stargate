use std::time::{SystemTime, UNIX_EPOCH};

/// Time source used by issuance and verification.
pub trait Clock: Send + Sync {
    fn unix_timestamp(&self) -> i64;
}

/// System UTC clock without process-global state.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn unix_timestamp(&self) -> i64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
            Err(error) => -i64::try_from(error.duration().as_secs()).unwrap_or(i64::MAX),
        }
    }
}

/// Source for a unique per-network-dispatch JTI.
pub trait DispatchIdSource: Send + Sync {
    fn next_dispatch_id(&self) -> String;
}

/// Monotonic-friendly ULID source for normal runtime use.
#[derive(Debug, Clone, Copy, Default)]
pub struct UlidDispatchIdSource;

impl DispatchIdSource for UlidDispatchIdSource {
    fn next_dispatch_id(&self) -> String {
        ulid::Ulid::new().to_string()
    }
}
