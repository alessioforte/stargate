//! Durable quota tracker: calendar windows with a balance in SQL.
//!
//! Each replica reserves a batch of units with one atomic ledger statement and
//! consumes it in memory. The hot path therefore performs no store round trip:
//! a reservation happens once per batch, and the batch shrinks as the window
//! approaches its limit so a crash cannot strand a large unused allowance.
//! Reserved units count as used even if the replica crashes before spending
//! them, so concurrent replicas can never exceed the limit.
//!
//! The batches belong to the process, not to one configuration generation:
//! [`DurableQuotas`] keeps them while a tracker of the same named quota
//! is alive, so publishing a configuration keeps spending them. A reservation
//! runs in its own task and can finish after the request's deadline. Each
//! ledger call has its own deadline so a stalled ledger releases the bucket.

use super::calendar::{CalendarWindow, WindowBounds};
use super::ledger::{QuotaBalance, QuotaLedger, Reservation, ReservationRequest};
use crate::clock::CachedClock;
use crate::decision::RateLimitDecision;
use crate::error::{RateLimitError, Result};
use crate::strategies::RateLimit;
use async_trait::async_trait;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// Upper bound on one reservation. Larger batches amortize the SQL statement
/// further but increase the units a crash can strand; at typical request rates
/// this still costs only a few statements per second per replica.
const MAX_LEASE_UNITS: u64 = 1024;
// Keep the bucket lock bounded even after the request's traffic budget expires.
const RESERVATION_TIMEOUT: Duration = Duration::from_secs(5);

/// Durable quota state of one process: the ledger and the batches this
/// replica holds in it, grouped by named limit.
pub struct DurableQuotas {
    ledger: Arc<dyn QuotaLedger>,
    /// A set lives while a tracker of its limit does, so a limit
    /// that leaves the configuration releases its leases.
    sets: std::sync::Mutex<HashMap<String, Weak<LeaseSet>>>,
}

impl DurableQuotas {
    pub fn new(ledger: Arc<dyn QuotaLedger>) -> Self {
        Self {
            ledger,
            sets: Default::default(),
        }
    }

    fn leases(&self, name: &str) -> Arc<LeaseSet> {
        let mut sets = self.sets.lock().expect("Lease sets lock poisoned");
        if let Some(set) = sets.get(name).and_then(Weak::upgrade) {
            return set;
        }
        sets.retain(|_, set| set.strong_count() > 0);
        let set = Arc::new(LeaseSet::default());
        sets.insert(name.to_owned(), Arc::downgrade(&set));
        set
    }
}

/// The leases of one named limit, by bucket.
#[derive(Default)]
struct LeaseSet {
    leases: DashMap<String, Arc<Mutex<Lease>>>,
    /// Start of the newest window seen. The first request of a new window
    /// drops the leases of earlier ones, once.
    window_start: AtomicI64,
    #[cfg(test)]
    prunes: std::sync::atomic::AtomicUsize,
}

impl LeaseSet {
    fn lease(&self, bucket: &str, window: WindowBounds) -> Arc<Mutex<Lease>> {
        if self.window_start.fetch_max(window.start, Ordering::Relaxed) < window.start {
            self.prune_before(window.start);
        }
        if let Some(lease) = self.leases.get(bucket) {
            return Arc::clone(lease.value());
        }
        Arc::clone(
            self.leases
                .entry(bucket.to_owned())
                .or_insert_with(|| Arc::new(Mutex::new(Lease::new(window))))
                .value(),
        )
    }

    fn prune_before(&self, window_start: i64) {
        #[cfg(test)]
        self.prunes.fetch_add(1, Ordering::Relaxed);
        // A lease in use stays; the next window drops it.
        self.leases.retain(|_, lease| {
            lease
                .try_lock()
                .map_or(true, |lease| lease.window.end > window_start)
        });
    }
}

struct Lease {
    window: WindowBounds,
    /// Units this replica has reserved in the window.
    granted: u64,
    /// Units this replica has spent from `granted`.
    consumed: u64,
    /// Balance in use the ledger last reported, shared by every replica.
    global_used: u64,
}

impl Lease {
    fn new(window: WindowBounds) -> Self {
        Self {
            window,
            granted: 0,
            consumed: 0,
            global_used: 0,
        }
    }

    fn observe(&mut self, balance: QuotaBalance) {
        if self.window != balance.window {
            *self = Self::new(balance.window);
        }
        self.global_used = balance.used;
    }

    fn reset_after(&self, now: i64) -> Option<Duration> {
        Some(Duration::from_secs(
            u64::try_from(self.window.end.saturating_sub(now)).unwrap_or(0),
        ))
    }

    fn unspent(&self) -> u64 {
        self.granted - self.consumed
    }

    fn remaining(&self, limit: u64) -> u64 {
        // `global_used` already counts this replica's unspent units.
        limit
            .saturating_sub(self.global_used)
            .saturating_add(self.unspent())
            .min(limit)
    }
}

pub struct DurableQuotaTracker {
    ledger: Arc<dyn QuotaLedger>,
    leases: Arc<LeaseSet>,
    clock: Arc<CachedClock>,
    limit: u64,
    window: CalendarWindow,
    name: Arc<str>,
    fingerprint: Arc<str>,
    config_version: i64,
}

impl DurableQuotaTracker {
    pub fn new(
        quotas: &DurableQuotas,
        name: &str,
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
            ledger: Arc::clone(&quotas.ledger),
            leases: quotas.leases(name),
            clock,
            limit,
            name: name.into(),
            fingerprint: window.fingerprint(limit).into(),
            window,
            config_version: 0,
        })
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

/// One reservation, independent of the request's deadline. The ledger may
/// already have charged the batch, so record it for the next requests to spend
/// if the ledger responds in time.
struct Reserve {
    ledger: Arc<dyn QuotaLedger>,
    bucket: String,
    name: Arc<str>,
    fingerprint: Arc<str>,
    config_version: i64,
    now: i64,
    window: WindowBounds,
    limit: u64,
    units: u64,
    cost: u64,
}

impl Reserve {
    /// Charge a batch and spend `cost` even if the request was cancelled.
    async fn run(self, mut lease: OwnedMutexGuard<Lease>) -> Result<RateLimitDecision> {
        let mut units = self.units;
        let mut reservation = self.reserve(units).await?;
        // The headroom came from a stale balance: another replica may have
        // reserved it first. Retry with the minimum that admits this request
        // when the refused balance still has room for it.
        if let Reservation::Refused(balance) = reservation
            && balance.window.contains(self.now)
            && units > self.cost
            && self.limit.saturating_sub(balance.used) >= self.cost
        {
            units = self.cost;
            reservation = self.reserve(units).await?;
        }
        match reservation {
            Reservation::Granted(balance) => {
                lease.observe(balance);
                lease.granted += units;
                lease.consumed += self.cost;
                Ok(RateLimitDecision::allowed(
                    self.limit,
                    lease.remaining(self.limit),
                    lease.reset_after(self.now),
                ))
            }
            Reservation::Refused(balance) => {
                lease.observe(balance);
                let reset = lease.reset_after(self.now);
                Ok(RateLimitDecision::denied(self.limit, 0, reset, reset))
            }
        }
    }

    async fn reserve(&self, units: u64) -> Result<Reservation> {
        tokio::time::timeout(
            RESERVATION_TIMEOUT,
            self.ledger.reserve(ReservationRequest {
                bucket: &self.bucket,
                name: &self.name,
                fingerprint: &self.fingerprint,
                config_version: self.config_version,
                now: self.now,
                window: self.window,
                limit: self.limit,
                units,
            }),
        )
        .await
        .map_err(|_| {
            RateLimitError::BackendError(format!(
                "quota reservation timed out after {RESERVATION_TIMEOUT:?}"
            ))
        })?
    }
}

#[async_trait]
impl RateLimit for DurableQuotaTracker {
    fn kind(&self) -> crate::strategies::LimitKind {
        crate::strategies::LimitKind::Quota
    }

    fn is_durable(&self) -> bool {
        true
    }

    fn set_quota_config_version(&mut self, version: i64) {
        self.config_version = version;
    }

    fn quota_fingerprint(&self) -> Option<&str> {
        Some(&self.fingerprint)
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

        let mut lease = self.leases.lease(key, bounds).lock_owned().await;
        if now < lease.window.start {
            let reset = lease.reset_after(now);
            return Ok(RateLimitDecision::denied(self.limit, 0, reset, reset));
        }
        if now >= lease.window.end {
            *lease = Lease::new(bounds);
        }
        let reset = lease.reset_after(now);
        if lease.unspent() >= cost {
            lease.consumed += cost;
            return Ok(RateLimitDecision::allowed(
                self.limit,
                lease.remaining(self.limit),
                reset,
            ));
        }
        let Some(units) = lease_units(self.limit, lease.global_used, cost) else {
            return Ok(RateLimitDecision::denied(self.limit, 0, reset, reset));
        };

        let reservation = Reserve {
            ledger: Arc::clone(&self.ledger),
            bucket: key.to_owned(),
            name: self.name.clone(),
            fingerprint: self.fingerprint.clone(),
            config_version: self.config_version,
            now,
            window: bounds,
            limit: self.limit,
            units,
            cost,
        };
        tokio::spawn(reservation.run(lease))
            .await
            .map_err(|error| {
                RateLimitError::BackendError(format!("quota reservation task failed: {error}"))
            })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::quota_tracker::calendar::CalendarPeriod;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::Notify;

    /// A ledger with the SQL repository's semantics, in memory.
    #[derive(Default)]
    struct TestLedger {
        state: std::sync::Mutex<HashMap<String, QuotaBalance>>,
        reservations: AtomicUsize,
        delay: Duration,
        first_response_stalled: Option<Notify>,
    }

    impl TestLedger {
        fn used(&self, bucket: &str) -> u64 {
            self.state
                .lock()
                .unwrap()
                .get(bucket)
                .map_or(0, |entry| entry.used)
        }
    }

    #[async_trait]
    impl QuotaLedger for TestLedger {
        async fn reserve(&self, request: ReservationRequest<'_>) -> Result<Reservation> {
            let attempt = self.reservations.fetch_add(1, Ordering::SeqCst);
            let reservation = {
                let mut state = self.state.lock().unwrap();
                let entry = state
                    .entry(request.bucket.to_string())
                    .or_insert(QuotaBalance {
                        window: request.window,
                        used: 0,
                    });
                if request.now >= entry.window.end {
                    *entry = QuotaBalance {
                        window: WindowBounds {
                            start: request.window.start.max(entry.window.end),
                            end: request.window.end,
                        },
                        used: 0,
                    };
                }
                if !entry.window.contains(request.now) || entry.used > request.limit - request.units
                {
                    Reservation::Refused(*entry)
                } else {
                    entry.used += request.units;
                    Reservation::Granted(*entry)
                }
            };
            // Like SQL, the charge is applied before the response arrives.
            if attempt == 0
                && let Some(stalled) = &self.first_response_stalled
            {
                stalled.notify_one();
                std::future::pending::<()>().await;
            }
            tokio::time::sleep(self.delay).await;
            Ok(reservation)
        }
    }

    fn day() -> CalendarWindow {
        CalendarWindow::parse(CalendarPeriod::Day, "UTC", None).unwrap()
    }

    fn tracker(ledger: Arc<TestLedger>, limit: u64) -> DurableQuotaTracker {
        replica(&DurableQuotas::new(ledger), limit)
    }

    fn replica(quotas: &DurableQuotas, limit: u64) -> DurableQuotaTracker {
        DurableQuotaTracker::new(quotas, "daily", CachedClock::new(), limit, day()).unwrap()
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
            ledger.used("bucket"),
            100,
            "reserved units must match the limit"
        );
    }

    #[tokio::test]
    async fn a_crashed_lease_strands_at_most_one_batch() {
        let ledger = Arc::new(TestLedger::default());
        let first = tracker(ledger.clone(), 1000);
        assert!(first.check("bucket", 1).await.unwrap().allowed);
        // Dropping the tracker and its process state simulates a crash
        // mid-batch: the reservation stays in the ledger, the lease is gone.
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
        assert_eq!(ledger.used("bucket"), 100);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_replicas_admit_the_limit_and_never_more() {
        const LIMIT: u64 = 1000;
        const REPLICAS: usize = 4;
        let ledger = Arc::new(TestLedger::default());
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..REPLICAS {
            // Every replica has its own process state over one shared ledger.
            let tracker = tracker(ledger.clone(), LIMIT);
            tasks.spawn(async move {
                let mut allowed = 0u64;
                while tracker.check("bucket", 1).await.unwrap().allowed {
                    allowed += 1;
                }
                allowed
            });
        }
        let mut allowed = 0;
        while let Some(result) = tasks.join_next().await {
            allowed += result.unwrap();
        }
        // A replica is denied only once the ledger is full, and the others
        // spend their own batches before they are: nothing is stranded.
        let batch = (LIMIT / 100).clamp(1, MAX_LEASE_UNITS);
        assert!(allowed <= LIMIT, "allowed {allowed}");
        assert!(
            allowed >= LIMIT - REPLICAS as u64 * batch,
            "allowed {allowed}"
        );
        assert_eq!(ledger.used("bucket"), LIMIT);
    }

    #[tokio::test]
    async fn an_exhausted_quota_is_denied_in_memory_after_one_refusal() {
        let ledger = Arc::new(TestLedger::default());
        let first = tracker(ledger.clone(), 100);
        while first.check("bucket", 1).await.unwrap().allowed {}
        let reservations = ledger.reservations.load(Ordering::SeqCst);

        // A restarted replica learns the balance from its first refusal.
        let restarted = tracker(ledger.clone(), 100);
        for _ in 0..100 {
            assert!(!restarted.check("bucket", 1).await.unwrap().allowed);
        }
        assert_eq!(ledger.reservations.load(Ordering::SeqCst), reservations + 1);
    }

    #[tokio::test]
    async fn a_stale_replica_does_not_retry_a_newer_window() {
        let ledger = Arc::new(TestLedger::default());
        let tracker = tracker(ledger.clone(), 100);
        let today = tracker.window.bounds(tracker.clock.now_secs()).unwrap();
        // Another replica already charged the next window.
        ledger.state.lock().unwrap().insert(
            "bucket".into(),
            QuotaBalance {
                window: WindowBounds {
                    start: today.end,
                    end: today.end + 86_400,
                },
                used: 1,
            },
        );
        for _ in 0..10 {
            assert!(!tracker.check("bucket", 1).await.unwrap().allowed);
        }
        assert_eq!(ledger.reservations.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn republished_trackers_keep_spending_the_same_batch() {
        const GENERATIONS: u64 = 10;
        let ledger = Arc::new(TestLedger::default());
        let quotas = DurableQuotas::new(ledger.clone());
        let mut active = replica(&quotas, 1000);
        for _ in 0..GENERATIONS {
            assert!(active.check("bucket", 1).await.unwrap().allowed);
            // Publication keeps the old balance even when the calendar changes.
            active = DurableQuotaTracker::new(
                &quotas,
                "daily",
                CachedClock::new(),
                1000,
                CalendarWindow::parse(CalendarPeriod::Month, "UTC", None).unwrap(),
            )
            .unwrap();
        }
        // One batch of 10 served every generation: nothing was stranded.
        assert_eq!(ledger.used("bucket"), 10);
        assert_eq!(ledger.reservations.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_cancelled_request_does_not_strand_its_reservation() {
        let ledger = Arc::new(TestLedger {
            delay: Duration::from_millis(50),
            ..TestLedger::default()
        });
        let tracker = tracker(ledger.clone(), 1000);
        let cancelled =
            tokio::time::timeout(Duration::from_millis(10), tracker.check("bucket", 1)).await;
        assert!(
            cancelled.is_err(),
            "the reservation must outlive the budget"
        );

        // The next request waits for the reservation and spends its batch.
        let decision = tracker.check("bucket", 1).await.unwrap();
        assert!(decision.allowed);
        assert_eq!(ledger.reservations.load(Ordering::SeqCst), 1);
        assert_eq!(ledger.used("bucket"), 10);
        // The cancelled request spent its unit too: 1000 - 10 + (10 - 2).
        assert_eq!(decision.remaining, 998);
    }

    #[tokio::test(start_paused = true)]
    async fn a_timed_out_reservation_releases_the_bucket() {
        let ledger = Arc::new(TestLedger {
            first_response_stalled: Some(Notify::new()),
            ..TestLedger::default()
        });
        let tracker = tracker(ledger.clone(), 1000);
        let first_check = tokio::time::timeout(
            RESERVATION_TIMEOUT + Duration::from_secs(1),
            tracker.check("bucket", 1),
        );
        let advance_past_deadline = async {
            let stalled = ledger.first_response_stalled.as_ref().unwrap();
            tokio::time::timeout(Duration::from_secs(1), stalled.notified())
                .await
                .expect("the first reservation must reach the ledger");
            tokio::time::advance(RESERVATION_TIMEOUT + Duration::from_millis(1)).await;
        };
        let (first, ()) = tokio::join!(first_check, advance_past_deadline);
        assert!(matches!(
            first.unwrap(),
            Err(RateLimitError::BackendError(_))
        ));
        assert_eq!(ledger.used("bucket"), 10);

        let decision = tokio::time::timeout(RESERVATION_TIMEOUT, tracker.check("bucket", 1))
            .await
            .expect("the timed-out reservation must release the bucket")
            .unwrap();
        assert!(decision.allowed);
        assert_eq!(ledger.reservations.load(Ordering::SeqCst), 2);
        assert_eq!(ledger.used("bucket"), 20);
        assert_eq!(decision.remaining, 989);
    }

    #[test]
    fn existing_leases_are_found_without_pruning() {
        let leases = LeaseSet::default();
        let window = WindowBounds {
            start: 1_000,
            end: 2_000,
        };
        for index in 0..10_000 {
            leases.lease(&format!("bucket-{index}"), window);
        }
        for _ in 0..1_000 {
            leases.lease("bucket-0", window);
        }
        assert_eq!(leases.prunes.load(Ordering::Relaxed), 1);
        assert_eq!(leases.leases.len(), 10_000);

        // The first request of the next window drops the earlier leases once.
        let next = WindowBounds {
            start: 2_000,
            end: 3_000,
        };
        leases.lease("bucket-0", next);
        leases.lease("bucket-1", next);
        assert_eq!(leases.prunes.load(Ordering::Relaxed), 2);
        assert_eq!(leases.leases.len(), 2);
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
