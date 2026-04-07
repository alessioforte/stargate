use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

pub struct CachedClock {
    /// Cached timestamp in microseconds since UNIX epoch
    micros: AtomicU64,
}

impl CachedClock {
    pub fn new() -> Arc<Self> {
        let clock = Arc::new(Self {
            micros: AtomicU64::new(Self::system_now_micros()),
        });

        let clock_weak = Arc::downgrade(&clock);
        std::thread::spawn(move || {
            let interval = std::time::Duration::from_millis(10);
            let mut next_tick = std::time::Instant::now() + interval;
            loop {
                let Some(clock) = clock_weak.upgrade() else {
                    break;
                };
                let now = std::time::Instant::now();
                if next_tick > now {
                    std::thread::sleep(next_tick - now);
                }
                clock
                    .micros
                    .store(Self::system_now_micros(), Ordering::Relaxed);
                next_tick += interval;
            }
        });

        clock
    }

    #[inline]
    pub fn now_micros(&self) -> u64 {
        self.micros.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn now_millis(&self) -> u64 {
        self.now_micros() / 1_000
    }

    #[inline]
    pub fn now_secs(&self) -> u64 {
        self.now_micros() / 1_000_000
    }

    fn system_now_micros() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
    }
}
