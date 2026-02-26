use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};

//
//       success            3 consecutive failures
// ┌──── Closed ────────────────────► Open ──────┐
// │       ▲                           │         │
// │       │ success          30s cooldown       │ failure
// │       │                           │         │
// │       └─────── HalfOpen ◄─────────┘         │
// │                    │                        │
// │                    └── failure ──► Open ◄───┘
// └─────────────────────────────────────────────┘
//

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
    /// Timestamp (epoch secs) when the circuit was opened
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
    /// HalfOpen → available (one probe allowed)
    /// Open → available only if cooldown has elapsed (transitions to HalfOpen)
    pub fn is_available(&self) -> bool {
        match self.state.load(Ordering::Acquire) {
            CB_CLOSED | CB_HALF_OPEN => true,
            CB_OPEN => {
                let opened = self.opened_at.load(Ordering::Relaxed);
                if Self::now_secs().saturating_sub(opened) >= self.cooldown_secs {
                    // Cooldown elapsed — transition to HalfOpen so one request probes
                    let _ = self.state.compare_exchange(
                        CB_OPEN,
                        CB_HALF_OPEN,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    );
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Record a success. Resets the circuit to Closed.
    pub fn record_success(&self) {
        self.fail_count.store(0, Ordering::Relaxed);
        self.state.store(CB_CLOSED, Ordering::Release);
    }

    /// Record a failure. After `fail_threshold` consecutive failures, opens the circuit.
    pub fn record_failure(&self) {
        let count = self.fail_count.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= self.fail_threshold {
            self.state.store(CB_OPEN, Ordering::Release);
            self.opened_at.store(Self::now_secs(), Ordering::Relaxed);
        }
    }

    pub fn state(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    pub fn fail_count(&self) -> usize {
        self.fail_count.load(Ordering::Relaxed)
    }
}
