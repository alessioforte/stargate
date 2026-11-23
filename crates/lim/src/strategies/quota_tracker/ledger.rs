//! Durable quota balance owned by the application.
//!
//! A durable quota keeps its balance in SQL, outside the state store, so a
//! flushed Redis or a restarted edge process does not reset it. The tracker
//! reserves units ahead of time and consumes them in memory; the ledger only
//! sees the reservations, never one call per request.

use super::calendar::WindowBounds;
use crate::error::Result;
use async_trait::async_trait;

/// The persisted bounds may differ from the requested calendar while its
/// previous window is still active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaBalance {
    pub window: WindowBounds,
    pub used: u64,
}

pub struct ReservationRequest<'a> {
    pub bucket: &'a str,
    pub config_version: i64,
    pub now: i64,
    pub window: WindowBounds,
    pub limit: u64,
    pub units: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reservation {
    Granted(QuotaBalance),
    Refused(QuotaBalance),
}

#[async_trait]
pub trait QuotaLedger: Send + Sync {
    /// Reserve from the persisted active window without exceeding the limit.
    /// Superseded versions and backend failures are errors.
    async fn reserve(&self, request: ReservationRequest<'_>) -> Result<Reservation>;
}
