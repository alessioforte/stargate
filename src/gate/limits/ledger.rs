//! SQL-backed ledger for calendar-window quotas.
use async_trait::async_trait;
use db::repo::{QuotaReservation, QuotaReserved};
use lim::{QuotaLedger, RateLimitError, Reservation};

pub(crate) struct SqlQuotaLedger {
    service: db::service::Service,
}

impl SqlQuotaLedger {
    pub(crate) fn new(service: db::service::Service) -> Self {
        Self { service }
    }
}

#[async_trait]
impl QuotaLedger for SqlQuotaLedger {
    async fn reserve(
        &self,
        bucket: &str,
        definition: &str,
        window_start: i64,
        window_end: i64,
        limit: u64,
        units: u64,
    ) -> lim::Result<Reservation> {
        let reserved = self
            .service
            .reserve_quota(QuotaReservation {
                bucket,
                definition,
                window_start,
                window_end,
                limit,
                units,
            })
            .await
            .map_err(|error| RateLimitError::BackendError(format!("quota ledger: {error:#}")))?;
        Ok(match reserved {
            QuotaReserved::Granted(used) => Reservation::Granted(used),
            QuotaReserved::Refused(used) => Reservation::Refused(used),
        })
    }
}
