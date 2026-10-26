use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

pub fn unix_now() -> Result<u64, SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

pub fn ttl_until(expires_at: u64, now: u64) -> u64 {
    expires_at.saturating_sub(now).max(1)
}
