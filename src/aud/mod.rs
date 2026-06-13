//! Audit subsystem.
//!
//! Audit events are written to the `audits` table inside the same database
//! transaction as the mutation they record — the table doubles as a
//! transactional outbox (see [`db::svc::Service`]). There is no in-process
//! buffering or async event bus: durability is the transaction's job.
//!
//! In cluster deployments a background [`relay`] tails the unpublished rows and
//! ships them to the message broker (Redis Streams). Edge deployments run no
//! relay at all, so [`spawn`] and [`shutdown`] are no-ops for them.

#[cfg(all(feature = "postgres", feature = "redis"))]
mod relay;

// pub mod sign;

/// Start the audit relay. Tails the audit outbox and ships rows to the broker in
/// cluster deployments; a no-op for edge deployments, which have no relay.
pub fn spawn() {
    #[cfg(all(feature = "postgres", feature = "redis"))]
    relay::spawn();
}

/// Stop the audit relay and wait for its in-flight batch to finish. A no-op for
/// edge deployments.
pub async fn shutdown() {
    #[cfg(all(feature = "postgres", feature = "redis"))]
    relay::shutdown().await;
}
