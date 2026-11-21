//! Durable quota tracker: calendar windows with a balance in SQL.
//!
//! Each replica reserves a batch of units with one atomic ledger statement and
//! consumes it in memory. The hot path therefore performs no store round trip:
//! a reservation happens once per batch, and the batch shrinks as the window
//! approaches its limit so a crash cannot strand a large unused allowance.
//! Reserved units count as used even if the replica crashes before spending
//! them, so concurrent replicas can never exceed the limit.

use super::calendar::{CalendarWindow, WindowBounds};
use super::ledger::QuotaLedger;
use crate::clock::CachedClock;
use crate::decision::RateLimitDecision;
use crate::error::{RateLimitError, Result};
use crate::strategies::RateLimit;
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Upper bound on one reservation. Larger batches amortize the SQL statement
/// further but increase the units a crash can strand; at typical request rates
/// this still costs only a few statements per second per replica.
const MAX_LEASE_UNITS: u64 = 1024;

/// Leases are bounded per process; stale windows are pruned when the map fills
/// so an address-keyed fallback with unbounded churn stays bounded too.
const MAX_TRACKED_LEASES: usize = 8192;

pub struct DurableQuotaTracker {
    ledger: Arc<dyn QuotaLedger>,
    clock: Arc<CachedClock>,
    limit: u64,
    window: CalendarWindow,
    /// Identity of the configured window; changing it starts a fresh balance.
    definition: String,
    leases: DashMap<String, Arc<Mutex<Lease>>>,
}

#[derive(Default)]
struct Lease {
    window_start: i64,
    window_end: i64,
    /// Units this replica has reserved in the current window.
    granted: u64,
    /// Units this replica has spent from `granted`.
    consumed: u64,
    /// Balance in use after the latest reservation, shared by every replica.
    global_used: u64,
}

impl DurableQuotaTracker {
    pub fn new(
        ledger: Arc<dyn QuotaLedger>,
        clock: Arc<CachedClock>,
        limit: u64,
        window: CalendarWindow,
    ) -> Result<Self> {
        if limit == 0 {
            return Err(RateLimitError::InvalidConfig(
                "Quota limit must be greater than zero".into(),
            ));
        }
        if limit > u64::try_from(i64::MAX).expect("i64::MAX fits in u64") {
            return Err(RateLimitError::InvalidConfig(
                "Quota limit must not exceed i64::MAX".into(),
            ));
        }
        Ok(Self {
            ledger,
            clock,
            limit,
            definition: window.definition(),
            window,
            leases: DashMap::new(),
        })
    }

    fn remaining(&self, lease: &Lease) -> u64 {
        // `global_used` already includes this replica's unused lease.
        self.limit
            .saturating_sub(lease.global_used)
            .saturating_add(lease.granted.saturating_sub(lease.consumed))
            .min(self.limit)
    }

    fn lease(&self, key: &str, window: WindowBounds) -> Arc<Mutex<Lease>> {
        if self.leases.len() >= MAX_TRACKED_LEASES {
            self.leases.retain(|_, lease| {
                lease
                    .try_lock()
                    .map_or(true, |lease| lease.window_end >= window.start)
            });
        }
        self.leases
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(Lease::default())))
            .clone()
    }
}

/// The next reservation size. It starts at 1% of the limit (capped), halves
/// with the remaining headroom near the limit, and never goes below the cost
/// of the request being admitted.
fn lease_units(limit: u64, used: u64, cost: u64) -> Option<u64> {
    let headroom = limit.saturating_sub(used);
    if headroom < cost {
        return None;
    }
    let base = (limit / 100).clamp(1, MAX_LEASE_UNITS);
    let near_limit = (headroom / 2).max(1);
    Some(cost.max(base.min(near_limit)).min(headroom))
}

#[async_trait]
impl RateLimit for DurableQuotaTracker {
    fn kind(&self) -> crate::strategies::LimitKind {
        crate::strategies::LimitKind::Quota
    }

    fn is_durable(&self) -> bool {
        true
    }

    async fn check(&self, key: &str, cost: u64) -> Result<RateLimitDecision> {
        let now_secs = self.clock.now_secs();
        let bounds = self
            .window
            .bounds(now_secs)
            .map_err(|error| RateLimitError::ClockError(error.to_string()))?;
        let now = i64::try_from(now_secs)
            .map_err(|_| RateLimitError::ClockError("timestamp out of range".into()))?;
        let reset = Some(Duration::from_secs(
            u64::try_from(bounds.end - now).unwrap_or(0),
        ));

        if cost > self.limit {
            return Ok(RateLimitDecision::denied(self.limit, 0, reset, reset));
        }
        if cost == 0 {
            return Ok(RateLimitDecision::allowed(self.limit, self.limit, reset));
        }

        let entry = self.lease(key, bounds);
        let mut lease = entry.lock().await;
        if lease.window_start != bounds.start {
            *lease = Lease {
                window_start: bounds.start,
                window_end: bounds.end,
                ..Lease::default()
            };
        }

        if lease.granted.saturating_sub(lease.consumed) >= cost {
            lease.consumed += cost;
            return Ok(RateLimitDecision::allowed(
                self.limit,
                self.remaining(&lease),
                reset,
            ));
        }

        let Some(mut units) = lease_units(self.limit, lease.global_used, cost) else {
            return Ok(RateLimitDecision::denied(self.limit, 0, reset, reset));
        };
        let mut reserved = self
            .ledger
            .reserve(
                key,
                &self.definition,
                bounds.start,
                bounds.end,
                self.limit,
                units,
            )
            .await?;
        // The headroom came from a stale balance: another replica may have
        // reserved it first. Retry with the minimum that admits this request.
        if reserved.is_none() && units > cost {
            units = cost;
            reserved = self
                .ledger
                .reserve(
                    key,
                    &self.definition,
                    bounds.start,
                    bounds.end,
                    self.limit,
                    units,
                )
                .await?;
        }
        let Some(global_used) = reserved else {
            return Ok(RateLimitDecision::denied(self.limit, 0, reset, reset));
        };

        lease.granted += units;
        lease.global_used = global_used;
        lease.consumed += cost;
        Ok(RateLimitDecision::allowed(
            self.limit,
            self.remaining(&lease),
            reset,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::quota_tracker::calendar::CalendarPeriod;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct TestLedger {
        state: std::sync::Mutex<HashMap<String, (i64, u64)>>,
        reservations: AtomicUsize,
    }

    #[async_trait]
    impl QuotaLedger for TestLedger {
        async fn reserve(
            &self,
            bucket: &str,
            _definition: &str,
            window_start: i64,
            _window_end: i64,
            limit: u64,
            units: u64,
        ) -> Result<Option<u64>> {
            self.reservations.fetch_add(1, Ordering::SeqCst);
            let mut state = self.state.lock().unwrap();
            let entry = state.entry(bucket.to_string()).or_insert((window_start, 0));
            if entry.0 != window_start {
                *entry = (window_start, 0);
            }
            if entry.1 + units > limit {
                return Ok(None);
            }
            entry.1 += units;
            Ok(Some(entry.1))
        }
    }

    fn tracker(ledger: Arc<TestLedger>, limit: u64) -> DurableQuotaTracker {
        DurableQuotaTracker::new(
            ledger,
            CachedClock::new(),
            limit,
            CalendarWindow::parse(CalendarPeriod::Day, "UTC", None).unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn batches_are_consumed_without_a_ledger_call_per_request() {
        let ledger = Arc::new(TestLedger::default());
        let tracker = tracker(ledger.clone(), 100_000);
        for _ in 0..500 {
            assert!(tracker.check("bucket", 1).await.unwrap().allowed);
        }
        // One reservation of 1% of the limit covers all 500 requests.
        assert_eq!(ledger.reservations.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_limit_is_never_exceeded_and_batches_shrink_near_it() {
        let ledger = Arc::new(TestLedger::default());
        let tracker = tracker(ledger.clone(), 100);
        let mut allowed = 0;
        for _ in 0..1000 {
            if tracker.check("bucket", 1).await.unwrap().allowed {
                allowed += 1;
            } else {
                break;
            }
        }
        assert_eq!(allowed, 100);
        assert_eq!(
            ledger.state.lock().unwrap().get("bucket").unwrap().1,
            100,
            "reserved units must match the limit"
        );
    }

    #[tokio::test]
    async fn a_crashed_lease_strands_at_most_one_batch() {
        let ledger = Arc::new(TestLedger::default());
        let first = tracker(ledger.clone(), 1000);
        assert!(first.check("bucket", 1).await.unwrap().allowed);
        // Dropping the tracker simulates a process crash mid-batch: the
        // reservation stays in the ledger, the in-memory lease disappears.
        drop(first);

        let second = tracker(ledger.clone(), 1000);
        let mut allowed = 0;
        while second.check("bucket", 1).await.unwrap().allowed {
            allowed += 1;
            assert!(allowed < 1000);
        }
        // 1000 - reserved-but-unused (at most one batch of 10) remains.
        assert!(allowed >= 1000 - 10, "allowed {allowed}");
        assert!(allowed <= 1000, "allowed {allowed}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_checks_share_reservations_within_the_limit() {
        let ledger = Arc::new(TestLedger::default());
        let tracker = Arc::new(tracker(ledger.clone(), 100));
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..16 {
            let tracker = tracker.clone();
            tasks.spawn(async move {
                let mut allowed = 0;
                for _ in 0..50 {
                    if tracker.check("bucket", 1).await.unwrap().allowed {
                        allowed += 1;
                    }
                }
                allowed
            });
        }
        let mut allowed = 0;
        while let Some(result) = tasks.join_next().await {
            allowed += result.unwrap();
        }
        assert_eq!(allowed, 100);
        assert_eq!(ledger.state.lock().unwrap().get("bucket").unwrap().1, 100);
    }

    #[tokio::test]
    async fn a_cost_above_the_limit_is_denied_without_a_reservation() {
        let ledger = Arc::new(TestLedger::default());
        let tracker = tracker(ledger.clone(), 10);
        let denied = tracker.check("bucket", 11).await.unwrap();
        assert!(!denied.allowed);
        assert_eq!(ledger.reservations.load(Ordering::SeqCst), 0);
    }
}
