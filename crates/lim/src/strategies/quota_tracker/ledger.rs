//! Durable quota balance owned by the application.
//!
//! A durable quota keeps its balance in SQL, outside the state store, so a
//! flushed Redis or a restarted edge process does not reset it. The tracker
//! reserves units ahead of time and consumes them in memory; the ledger only
//! sees the reservations, never one call per request.

use crate::error::Result;
use async_trait::async_trait;

#[async_trait]
pub trait QuotaLedger: Send + Sync {
    /// Atomically reserve up to `units` in `bucket` for the window
    /// `[window_start, window_end)`. Returns the balance in use after the
    /// reservation, or `None` when the remaining balance cannot fund
    /// `units`. Backend failures are errors: a durable quota never fails open.
    async fn reserve(
        &self,
        bucket: &str,
        definition: &str,
        window_start: i64,
        window_end: i64,
        limit: u64,
        units: u64,
    ) -> Result<Option<u64>>;
}
