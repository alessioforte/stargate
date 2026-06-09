use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};

//
//       success            N consecutive failures
// ┌──── Closed ────────────────────► Open ──────┐
// │       ▲                           │         │
// │       │ success      cooldown elapsed       │ failure
// │       │                           │         │
// │       └─────── HalfOpen ◄─────────┘         │
// │                    │                        │
// │                    └── failure ──► Open ◄───┘
// └─────────────────────────────────────────────┘
//
// HalfOpen admits a single probe request at a time. Concurrent callers are
// rejected until the probe reports success (→ Closed) or failure (→ Open). If a
// probe never reports back, the gate re-arms after another cooldown window so a
// lost probe cannot wedge the circuit permanently.

// Circuit breaker states
const CB_CLOSED: u8 = 0; // healthy — all requests allowed
const CB_OPEN: u8 = 1; // tripped — requests rejected
const CB_HALF_OPEN: u8 = 2; // probing — one request allowed to test recovery

pub struct CircuitBreaker {
    state: AtomicU8,
    fail_count: AtomicUsize,
    fail_threshold: usize,
    /// Cooldown in seconds before transitioning from Open → HalfOpen
    cooldown_secs: u64,
    /// Timestamp (epoch secs) when the circuit opened, or when the current
    /// half-open probe window started.
    opened_at: AtomicU64,
}

impl CircuitBreaker {
    pub fn new(fail_threshold: usize, cooldown_secs: u64) -> Self {
        Self {
            state: AtomicU8::new(CB_CLOSED),
            fail_count: AtomicUsize::new(0),
            fail_threshold,
            cooldown_secs,
            opened_at: AtomicU64::new(0),
        }
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Returns true if the upstream should receive traffic.
    /// Closed → always available
    /// Open → available only once cooldown elapses (transitions to HalfOpen)
    /// HalfOpen → available to a single probe at a time
    pub fn is_available(&self) -> bool {
        self.is_available_at(Self::now_secs())
    }

    fn is_available_at(&self, now: u64) -> bool {
        match self.state.load(Ordering::Acquire) {
            CB_CLOSED => true,
            CB_HALF_OPEN => {
                // A probe is in flight. Admit another only if the previous one
                // stalled past the cooldown, so a lost probe can't wedge the
                // circuit. Exactly one caller wins the re-arm.
                let opened = self.opened_at.load(Ordering::Relaxed);
                if now.saturating_sub(opened) >= self.cooldown_secs {
                    self.opened_at
                        .compare_exchange(opened, now, Ordering::AcqRel, Ordering::Relaxed)
                        .is_ok()
                } else {
                    false
                }
            }
            CB_OPEN => {
                let opened = self.opened_at.load(Ordering::Relaxed);
                if now.saturating_sub(opened) >= self.cooldown_secs {
                    // Cooldown elapsed: exactly one caller flips Open → HalfOpen
                    // and is admitted as the probe.
                    if self
                        .state
                        .compare_exchange(
                            CB_OPEN,
                            CB_HALF_OPEN,
                            Ordering::AcqRel,
                            Ordering::Relaxed,
                        )
                        .is_ok()
                    {
                        // Start the half-open probe window's stall timer.
                        self.opened_at.store(now, Ordering::Relaxed);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Record a success. Resets the circuit to Closed.
    pub fn record_success(&self) {
        // Fast path: steady-state success stays read-only to avoid cache-line
        // write contention when many cores report success concurrently.
        if self.state.load(Ordering::Relaxed) == CB_CLOSED
            && self.fail_count.load(Ordering::Relaxed) == 0
        {
            return;
        }
        self.fail_count.store(0, Ordering::Relaxed);
        self.state.store(CB_CLOSED, Ordering::Release);
    }

    /// Record a failure. Opens the circuit after `fail_threshold` consecutive
    /// failures, or immediately if a half-open probe fails.
    pub fn record_failure(&self) {
        self.record_failure_at(Self::now_secs());
    }

    fn record_failure_at(&self, now: u64) {
        // A failed half-open probe re-opens the circuit immediately.
        if self.state.load(Ordering::Acquire) == CB_HALF_OPEN {
            self.opened_at.store(now, Ordering::Relaxed);
            self.state.store(CB_OPEN, Ordering::Release);
            return;
        }
        let count = self.fail_count.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= self.fail_threshold {
            self.opened_at.store(now, Ordering::Relaxed);
            self.state.store(CB_OPEN, Ordering::Release);
        }
    }

    pub fn state(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    pub fn fail_count(&self) -> usize {
        self.fail_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_after_consecutive_failures() {
        let cb = CircuitBreaker::new(3, 60);
        cb.record_failure_at(100);
        cb.record_failure_at(100);
        assert_eq!(cb.state(), CB_CLOSED);
        assert!(cb.is_available_at(100));
        cb.record_failure_at(100); // 3rd consecutive → open
        assert_eq!(cb.state(), CB_OPEN);
        assert!(!cb.is_available_at(100));
    }

    #[test]
    fn rejects_until_cooldown_then_admits_single_probe() {
        let cb = CircuitBreaker::new(1, 10);
        cb.record_failure_at(100); // open at t=100
        assert!(!cb.is_available_at(105)); // 5s < 10s cooldown
        // cooldown elapsed: first caller is admitted and transitions to half-open
        assert!(cb.is_available_at(110));
        assert_eq!(cb.state(), CB_HALF_OPEN);
        // concurrent callers rejected while the probe is in flight
        assert!(!cb.is_available_at(111));
        assert!(!cb.is_available_at(115));
    }

    #[test]
    fn half_open_success_closes_circuit() {
        let cb = CircuitBreaker::new(1, 10);
        cb.record_failure_at(100);
        assert!(cb.is_available_at(110)); // → half-open probe admitted
        cb.record_success();
        assert_eq!(cb.state(), CB_CLOSED);
        assert_eq!(cb.fail_count(), 0);
        assert!(cb.is_available_at(111));
    }

    #[test]
    fn half_open_failure_reopens_circuit() {
        let cb = CircuitBreaker::new(1, 10);
        cb.record_failure_at(100);
        assert!(cb.is_available_at(110)); // half-open
        cb.record_failure_at(110); // probe failed → reopen immediately
        assert_eq!(cb.state(), CB_OPEN);
        assert!(!cb.is_available_at(115)); // fresh cooldown
        assert!(cb.is_available_at(120)); // cooldown elapsed → probe again
    }

    #[test]
    fn half_open_rearms_after_stalled_probe() {
        let cb = CircuitBreaker::new(1, 10);
        cb.record_failure_at(100);
        assert!(cb.is_available_at(110)); // half-open, probe admitted
        // probe never reports back; gate stays closed within the window
        assert!(!cb.is_available_at(115));
        assert!(!cb.is_available_at(119));
        // window elapsed → re-arm exactly one fresh probe
        assert!(cb.is_available_at(120));
        assert!(!cb.is_available_at(121));
    }

    #[test]
    fn success_resets_failure_count() {
        let cb = CircuitBreaker::new(3, 60);
        cb.record_failure_at(100);
        cb.record_failure_at(100);
        assert_eq!(cb.fail_count(), 2);
        cb.record_success();
        assert_eq!(cb.fail_count(), 0);
        assert_eq!(cb.state(), CB_CLOSED);
    }
}
