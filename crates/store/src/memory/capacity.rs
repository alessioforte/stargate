//! The memory bound of a [`super::MemoryStore`].
//!
//! Like Redis with `maxmemory-policy noeviction`, a full store evicts nothing
//! and refuses to create entries, so sessions and revocations are never lost to
//! make room. Updates of existing entries still apply, and revocations are
//! recorded even when the store is full: ending access must not wait for
//! memory. Callers see [`StoreError::Full`]: traffic limits follow their
//! `on_store_error` policy and security state fails closed, as with a full
//! Redis.
//!
//! Usage is an estimate kept without walking the map on the request path: the
//! sweep measures every live entry and the entries created since are added as
//! they are admitted. Deletions are credited at the next sweep, so between
//! sweeps the estimate errs towards full.

use crate::error::{StoreError, StoreResult};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Default)]
pub(crate) struct Capacity {
    /// Bytes the store may hold; zero leaves it unbounded.
    limit: usize,
    /// Bytes of the live entries the last sweep measured.
    measured: AtomicUsize,
    /// Bytes of the entries created since the last sweep.
    admitted: AtomicUsize,
}

impl Capacity {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }

    /// Reserves `bytes` for a new entry of `key`, or refuses it when the store
    /// would exceed its limit.
    pub(crate) fn admit(&self, key: &str, bytes: usize) -> StoreResult<()> {
        if self.limit == 0 {
            return Ok(());
        }
        if crate::keys::admitted_when_full(key) {
            self.admitted.fetch_add(bytes, Ordering::Relaxed);
            return Ok(());
        }
        self.admitted
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |admitted| {
                let used = self
                    .measured
                    .load(Ordering::Relaxed)
                    .saturating_add(admitted)
                    .saturating_add(bytes);
                (used <= self.limit).then_some(admitted + bytes)
            })
            .map(|_| ())
            .map_err(|_| StoreError::Full)
    }

    /// Estimated bytes in use.
    pub(crate) fn used(&self) -> usize {
        self.measured
            .load(Ordering::Relaxed)
            .saturating_add(self.admitted.load(Ordering::Relaxed))
    }

    pub(crate) fn limit(&self) -> usize {
        self.limit
    }

    /// Records a sweep that measured `measured` bytes after `admitted_before`
    /// bytes had been admitted. Entries admitted while the sweep ran stay
    /// counted, even those the sweep saw too: the estimate never falls short.
    pub(crate) fn measured(&self, measured: usize, admitted_before: usize) {
        self.measured.store(measured, Ordering::Relaxed);
        self.admitted
            .update(Ordering::Relaxed, Ordering::Relaxed, |admitted| {
                admitted.saturating_sub(admitted_before)
            });
    }

    /// Bytes admitted so far, read before a sweep starts.
    pub(crate) fn admitted(&self) -> usize {
        self.admitted.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::Capacity;
    use crate::error::StoreError;

    #[test]
    fn a_full_store_refuses_new_entries_but_records_revocations() {
        let capacity = Capacity::new(100);
        capacity
            .admit("stargate:v1:{ip:a}:ingress:gateway", 60)
            .unwrap();
        assert!(matches!(
            capacity.admit("stargate:v1:{ip:b}:ingress:gateway", 41),
            Err(StoreError::Full)
        ));
        capacity
            .admit("stargate:v1:{ip:b}:ingress:gateway", 40)
            .unwrap();
        assert_eq!(capacity.used(), 100);

        capacity.admit("stargate:v1:{j:jti}:revoked", 50).unwrap();
        assert_eq!(capacity.used(), 150);
        assert!(matches!(
            capacity.admit("stargate:v1:{ip:c}:ingress:gateway", 1),
            Err(StoreError::Full)
        ));
    }

    #[test]
    fn a_sweep_replaces_the_estimate_and_keeps_concurrent_admissions() {
        let capacity = Capacity::new(100);
        capacity.admit("a", 90).unwrap();
        let before = capacity.admitted();
        // Admitted while the sweep walked the map.
        capacity.admit("b", 5).unwrap();
        capacity.measured(20, before);
        assert_eq!(capacity.used(), 25);
        capacity.admit("c", 75).unwrap();
        assert!(capacity.admit("d", 1).is_err());
    }

    #[test]
    fn an_unbounded_store_admits_everything() {
        let capacity = Capacity::new(0);
        capacity.admit("a", usize::MAX).unwrap();
        capacity.admit("b", usize::MAX).unwrap();
    }
}
