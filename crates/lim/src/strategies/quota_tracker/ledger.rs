//! Durable quota balance owned by the application.
//!
//! A durable quota keeps its balance in SQL, outside the state store, so a
//! flushed Redis or a restarted edge process does not reset it. The tracker
//! reserves units ahead of time and consumes them in memory; the ledger only
//! sees the reservations, never one call per request.

use crate::error::Result;
use async_trait::async_trait;

/// Outcome of one reservation, with the balance in use that decided it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reservation {
    /// The units were charged; the balance in use afterwards.
    Granted(u64),
    /// The units do not fit in the balance in use. Within one window and
    /// definition the balance only grows, so the tracker can deny later
    /// requests in memory instead of asking again. A stored window newer than
    /// the requested one refuses the whole requested window and reports the
    /// limit.
    Refused(u64),
}

#[async_trait]
pub trait QuotaLedger: Send + Sync {
    /// Atomically reserve `units` in `bucket` for the window
    /// `[window_start, window_end)`, never beyond `limit`. Backend failures
    /// are errors: a durable quota never fails open.
    async fn reserve(
        &self,
        bucket: &str,
        definition: &str,
        window_start: i64,
        window_end: i64,
        limit: u64,
        units: u64,
    ) -> Result<Reservation>;
}
