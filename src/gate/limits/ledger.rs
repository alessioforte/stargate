//! SQL-backed ledger for calendar-window quotas.
use async_trait::async_trait;
use db::repo::{QUOTA_PRUNE_BATCH, QuotaOwner, QuotaReservation, QuotaReserved};
use lim::strategies::quota_tracker::WindowBounds;
use lim::{QuotaBalance, QuotaLedger, RateLimitError, Reservation, ReservationRequest};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// How often each replica removes the balances of deleted owners.
const RETENTION_INTERVAL: Duration = Duration::from_secs(600);

pub(crate) struct SqlQuotaLedger {
    service: db::service::Service,
}

impl SqlQuotaLedger {
    pub(crate) fn new(service: db::service::Service) -> Self {
        Self { service }
    }
}

/// The subject or organization a durable bucket belongs to. Buckets keyed by
/// a client address have none and never reach SQL.
fn owner(bucket: &str) -> lim::Result<QuotaOwner<'_>> {
    match store::keys::entity(bucket) {
        Some(("sub", id)) => Ok(QuotaOwner::Subject(id)),
        Some(("org", id)) => Ok(QuotaOwner::Organization(id)),
        _ => Err(RateLimitError::InvalidConfig(
            "a durable quota needs a subject or organization bucket".into(),
        )),
    }
}

#[async_trait]
impl QuotaLedger for SqlQuotaLedger {
    async fn reserve(&self, request: ReservationRequest<'_>) -> lim::Result<Reservation> {
        let reserved = self
            .service
            .reserve_quota(QuotaReservation {
                owner: owner(request.bucket)?,
                bucket: request.bucket,
                name: request.name,
                fingerprint: request.fingerprint,
                config_version: request.config_version,
                now: request.now,
                window_start: request.window.start,
                window_end: request.window.end,
                limit: request.limit,
                units: request.units,
            })
            .await
            .map_err(|error| RateLimitError::BackendError(format!("quota ledger: {error:#}")))?;
        let balance = |balance: db::repo::QuotaBalance| QuotaBalance {
            used: balance.used,
            window: WindowBounds {
                start: balance.window_start,
                end: balance.window_end,
            },
        };
        match reserved {
            QuotaReserved::Granted(value) => Ok(Reservation::Granted(balance(value))),
            QuotaReserved::Refused(value) => Ok(Reservation::Refused(balance(value))),
            QuotaReserved::StaleDefinition => Err(RateLimitError::BackendError(
                "quota reservation uses a superseded quota definition".into(),
            )),
        }
    }
}

/// Remove the durable balances of deleted subjects and organizations, in
/// batches, at startup and then every [`RETENTION_INTERVAL`] until shutdown.
pub(crate) async fn prune_orphaned_quotas(
    service: db::service::Service,
    shutdown: CancellationToken,
) {
    let mut interval = tokio::time::interval(RETENTION_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => return,
            _ = interval.tick() => {}
        }
        let mut total = 0;
        loop {
            let pruned = tokio::select! {
                biased;
                _ = shutdown.cancelled() => return,
                pruned = service.prune_orphaned_quotas() => pruned,
            };
            match pruned {
                Ok(count) => {
                    total += count;
                    if count < QUOTA_PRUNE_BATCH {
                        break;
                    }
                }
                Err(error) => {
                    tracing::warn!("Durable quota retention failed: {error:#}");
                    break;
                }
            }
        }
        if total > 0 {
            tracing::info!(
                count = total,
                "Durable quota balances of deleted owners pruned"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::owner;
    use db::repo::QuotaOwner;

    #[test]
    fn durable_buckets_belong_to_a_subject_or_an_organization() {
        let subject = format!("{}:monthly", store::keys::limits::subject_quota("user-1"));
        assert_eq!(owner(&subject).unwrap(), QuotaOwner::Subject("user-1"));
        let organization = format!("{}:monthly", store::keys::limits::org_quota("org-1"));
        assert_eq!(
            owner(&organization).unwrap(),
            QuotaOwner::Organization("org-1")
        );
        let address = format!(
            "{}:monthly",
            store::keys::limits::org_ip_quota("203.0.113.9")
        );
        assert!(owner(&address).is_err());
    }
}
