//! Audit subsystem.
//!
//! Audit events are written to `outbox_events` inside the same database
//! transaction as the mutation they record. There is no in-process buffering
//! or async event bus: durability is the transaction's job.
//!
//! In cluster deployments a background [`relay`] tails the unpublished rows and
//! ships them to the message broker (Redis Streams). Edge deployments run no
//! relay at all, so [`spawn`] and [`shutdown`] are no-ops for them.

#[cfg(all(feature = "postgres", feature = "redis"))]
mod relay;

// pub mod sign;

/// Start the audit relay. Tails the audit outbox and ships rows to the broker in
/// cluster deployments; a no-op for edge deployments, which have no relay.
pub fn spawn() -> std::io::Result<()> {
    #[cfg(all(feature = "postgres", feature = "redis"))]
    return relay::spawn();

    #[cfg(not(all(feature = "postgres", feature = "redis")))]
    Ok(())
}

/// Stop the audit relay and wait for its in-flight batch to finish. A no-op for
/// edge deployments.
pub async fn shutdown() {
    #[cfg(all(feature = "postgres", feature = "redis"))]
    relay::shutdown().await;
}

/// Delivery behavior exposed to the admin overview.
pub fn delivery_mode() -> &'static str {
    #[cfg(all(feature = "postgres", feature = "redis"))]
    {
        if relay::enabled() {
            "relay"
        } else {
            "disabled"
        }
    }

    #[cfg(not(all(feature = "postgres", feature = "redis")))]
    {
        "durable_only"
    }
}

#[cfg(all(test, feature = "edge"))]
mod tests {
    #[tokio::test]
    async fn audit_edge_relay_lifecycle_is_a_noop() {
        super::spawn().expect("edge relay start must be a no-op");
        super::shutdown().await;
    }
}
