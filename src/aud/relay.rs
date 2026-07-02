//! Audit outbox relay (cluster only).
//!
//! A single background task per process tails the `audits` table — the
//! transactional outbox — and ships unpublished rows to Redis Streams. Rows are
//! claimed with `FOR UPDATE SKIP LOCKED`, so several relay nodes can drain the
//! backlog concurrently without ever publishing the same row twice from the same
//! claim. Delivery is **at-least-once**: a crash between the stream append and
//! the `published_at` commit re-ships the batch on restart, so consumers must
//! dedupe on the audit `id`.

use crate::etc::telemetry;
use db::ent::OutboxAudit;
use db::service::Service;
use once_cell::sync::OnceCell;
use std::sync::Arc;
use std::time::{Duration, Instant};
use store::{RedisStore, StreamEntry};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tracing::Instrument;

const DEFAULT_BATCH: i64 = 256;
const DEFAULT_INTERVAL_MS: u64 = 1000;
const DEFAULT_STREAM: &str = "audit:events";
const DEFAULT_STREAM_MAXLEN: usize = 100_000;

/// Backoff floor applied after a failed batch (DB or broker error).
const BACKOFF_BASE: Duration = Duration::from_millis(500);
/// Backoff ceiling; the relay never sleeps longer than this between retries.
const BACKOFF_MAX: Duration = Duration::from_secs(30);

struct RelayHandle {
    shutdown: Arc<Notify>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

static RELAY: OnceCell<RelayHandle> = OnceCell::new();

/// Start the relay task, unless disabled via `AUDIT_RELAY_ENABLED=false`.
pub fn spawn() {
    if !relay_enabled() {
        tracing::info!("Audit relay disabled (AUDIT_RELAY_ENABLED=false)");
        return;
    }

    let shutdown = Arc::new(Notify::new());
    let handle = tokio::spawn(run(shutdown.clone()));
    let relay = RelayHandle {
        shutdown,
        handle: Mutex::new(Some(handle)),
    };

    if RELAY.set(relay).is_err() {
        tracing::error!("Audit relay already started");
    }
}

/// Signal the relay to stop and wait for the in-flight batch to finish.
///
/// Stopping mid-backlog is safe: unpublished rows stay in the table and are
/// re-claimed when a relay next runs.
pub async fn shutdown() {
    if let Some(relay) = RELAY.get() {
        relay.shutdown.notify_one();
        let mut guard = relay.handle.lock().await;
        if let Some(handle) = guard.take() {
            let _ = handle.await;
        }
        tracing::info!("Audit relay shut down");
    }
}

async fn run(shutdown: Arc<Notify>) {
    let svc = crate::db::service();
    let store = crate::etc::store::use_store();
    let stream = stream_key();
    let maxlen = stream_maxlen();
    let batch = batch_size();
    let idle = idle_interval();

    tracing::info!(
        stream = %stream,
        batch,
        idle_ms = idle.as_millis() as u64,
        maxlen = ?maxlen,
        "Audit relay started"
    );

    // `delay` is how long to wait before the next attempt: zero to drain a full
    // backlog as fast as the DB and broker allow, `idle` once caught up, and the
    // current `backoff` after a failure. The `biased` select polls shutdown
    // first every iteration, so even a continuous backlog never starves it.
    let mut delay = Duration::ZERO;
    let mut backoff = BACKOFF_BASE;

    loop {
        tokio::select! {
            biased;
            _ = shutdown.notified() => break,
            _ = tokio::time::sleep(delay) => {}
        }

        let started = Instant::now();
        let result = relay_once(&svc, store, &stream, maxlen, batch)
            .instrument(tracing::debug_span!(
                "audit.relay.batch",
                stream = %stream,
                batch,
            ))
            .await;

        match result {
            Ok(count) => {
                telemetry::record_audit_relay_batch("success", count, started.elapsed());
                backoff = BACKOFF_BASE;
                delay = if count as i64 >= batch {
                    Duration::ZERO
                } else {
                    idle
                };
            }
            Err(error) => {
                telemetry::record_audit_relay_batch("error", 0, started.elapsed());
                tracing::warn!(
                    %error,
                    backoff_ms = backoff.as_millis() as u64,
                    "Audit relay batch failed; backing off"
                );
                delay = backoff;
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    }

    tracing::info!("Audit relay stopped");
}

/// Claim one batch, ship it, and mark it published. Returns the number of rows
/// relayed (`0` when the backlog is empty).
async fn relay_once(
    svc: &Service,
    store: &RedisStore,
    stream: &str,
    maxlen: Option<usize>,
    batch: i64,
) -> anyhow::Result<usize> {
    let claim = svc.claim_unpublished_audits(batch).await?;
    if claim.is_empty() {
        return Ok(0);
    }

    let entries: Vec<StreamEntry> = claim.rows().iter().map(to_entry).collect();
    let count = entries.len();

    // Publish first, commit second. If the publish fails the claim is dropped
    // here (transaction rollback) and nothing is marked published; if the commit
    // fails the rows are simply re-claimed and re-shipped later.
    store.xadd_batch(stream, maxlen, &entries).await?;
    claim.commit_published().await?;

    Ok(count)
}

/// Map an outbox row to a stream entry: the full audit as a JSON `payload`, plus
/// `id` and `seq` as top-level fields for consumer dedup and ordering.
fn to_entry(row: &OutboxAudit) -> StreamEntry {
    let payload = serde_json::to_string(&row.audit).unwrap_or_else(|error| {
        tracing::error!(
            id = %row.audit.id,
            %error,
            "Failed to serialize audit for stream; shipping empty payload"
        );
        "{}".to_string()
    });

    StreamEntry::new(vec![
        ("id".to_string(), row.audit.id.clone()),
        ("seq".to_string(), row.seq.to_string()),
        ("payload".to_string(), payload),
    ])
}

fn relay_enabled() -> bool {
    std::env::var("AUDIT_RELAY_ENABLED")
        .ok()
        .map(|v| {
            !matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "false" | "0" | "no" | "off"
            )
        })
        .unwrap_or(true)
}

fn batch_size() -> i64 {
    std::env::var("AUDIT_RELAY_BATCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n: &i64| *n > 0)
        .unwrap_or(DEFAULT_BATCH)
}

fn idle_interval() -> Duration {
    let ms = std::env::var("AUDIT_RELAY_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n: &u64| *n > 0)
        .unwrap_or(DEFAULT_INTERVAL_MS);
    Duration::from_millis(ms)
}

fn stream_key() -> String {
    std::env::var("AUDIT_STREAM")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_STREAM.to_string())
}

/// `0` (or a non-numeric value) disables trimming (unbounded stream); any
/// positive value caps the stream to ~N most recent entries.
fn stream_maxlen() -> Option<usize> {
    match std::env::var("AUDIT_STREAM_MAXLEN")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        Some(0) => None,
        Some(n) => Some(n),
        None => Some(DEFAULT_STREAM_MAXLEN),
    }
}
