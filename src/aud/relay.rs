//! Audit outbox relay (cluster only).
//!
//! A single background task per process tails `outbox_events` and ships each
//! immutable stored payload to Redis Streams. Rows are claimed with `FOR UPDATE
//! SKIP LOCKED`, so several relay nodes can drain the backlog concurrently.
//! Delivery is **at-least-once**: a crash between the stream append and the
//! `published_at` commit re-ships the byte-identical payload, so consumers must
//! deduplicate on the `event_id` inside it.

use crate::etc::telemetry;
use db::ent::OutboxEventRow;
use db::service::Service;
use once_cell::sync::OnceCell;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use store::{RedisStore, StreamEntry};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tracing::Instrument;

const DEFAULT_BATCH: i64 = 256;
const DEFAULT_INTERVAL_MS: u64 = 1000;
const DEFAULT_STREAM: &str = "audit.raw";
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

#[derive(Debug, Clone, Eq, PartialEq)]
struct RelayConfig {
    stream: String,
    maxlen: Option<usize>,
    batch: i64,
    idle: Duration,
}

impl RelayConfig {
    fn from_env() -> io::Result<Self> {
        Self::from_lookup(env_value)
    }

    fn from_lookup(mut lookup: impl FnMut(&str) -> io::Result<Option<String>>) -> io::Result<Self> {
        let batch = match lookup("AUDIT_RELAY_BATCH")? {
            Some(value) => parse_positive_i64("AUDIT_RELAY_BATCH", &value)?,
            None => DEFAULT_BATCH,
        };
        let interval_ms = match lookup("AUDIT_RELAY_INTERVAL_MS")? {
            Some(value) => parse_positive_u64("AUDIT_RELAY_INTERVAL_MS", &value)?,
            None => DEFAULT_INTERVAL_MS,
        };
        let stream = match lookup("AUDIT_REDIS_STREAM")? {
            Some(value) => parse_stream("AUDIT_REDIS_STREAM", value)?,
            None => DEFAULT_STREAM.to_string(),
        };
        let maxlen = match lookup("AUDIT_STREAM_MAXLEN")? {
            Some(value) => Some(parse_usize("AUDIT_STREAM_MAXLEN", &value)?).filter(|n| *n > 0),
            None => Some(DEFAULT_STREAM_MAXLEN),
        };

        Ok(Self {
            stream,
            maxlen,
            batch,
            idle: Duration::from_millis(interval_ms),
        })
    }
}

/// Start the relay task, unless disabled via `AUDIT_RELAY_ENABLED=false`.
pub fn spawn() -> io::Result<()> {
    if !relay_enabled()? {
        tracing::info!("Audit relay disabled (AUDIT_RELAY_ENABLED=false)");
        return Ok(());
    }
    if RELAY.get().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "audit relay already started",
        ));
    }

    let config = RelayConfig::from_env()?;
    let shutdown = Arc::new(Notify::new());
    let handle = tokio::spawn(run(shutdown.clone(), config));
    let relay = RelayHandle {
        shutdown,
        handle: Mutex::new(Some(handle)),
    };

    if let Err(relay) = RELAY.set(relay) {
        if let Some(handle) = relay.handle.into_inner() {
            handle.abort();
        }
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "audit relay already started",
        ));
    }
    Ok(())
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

async fn run(shutdown: Arc<Notify>, config: RelayConfig) {
    let svc = crate::db::service();
    let store = crate::etc::store::use_store();

    tracing::info!(
        stream = %config.stream,
        batch = config.batch,
        idle_ms = config.idle.as_millis() as u64,
        maxlen = ?config.maxlen,
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
        let result = relay_once(&svc, store, &config.stream, config.maxlen, config.batch)
            .instrument(tracing::debug_span!(
                "audit.relay.batch",
                stream = %config.stream,
                batch = config.batch,
            ))
            .await;

        match result {
            Ok(count) => {
                telemetry::record_audit_relay_batch("success", count, started.elapsed());
                backoff = BACKOFF_BASE;
                delay = if count as i64 >= config.batch {
                    Duration::ZERO
                } else {
                    config.idle
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
    let claim = svc.claim_unpublished_outbox_events(batch).await?;
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

/// Map an outbox row to the shared Redis contract without parsing or rebuilding
/// its immutable raw-event payload.
fn to_entry(row: &OutboxEventRow) -> StreamEntry {
    StreamEntry::new(vec![("payload".to_string(), row.payload.clone())])
}

fn relay_enabled() -> io::Result<bool> {
    match env_value("AUDIT_RELAY_ENABLED")? {
        None => Ok(true),
        Some(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Ok(true),
            "false" | "0" | "no" | "off" => Ok(false),
            _ => Err(invalid_config(
                "AUDIT_RELAY_ENABLED",
                &value,
                "a boolean (true/false, 1/0, yes/no, or on/off)",
            )),
        },
    }
}

pub(super) fn enabled() -> bool {
    relay_enabled().unwrap_or(false)
}

fn env_value(name: &str) -> io::Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must contain valid Unicode data"),
        )),
    }
}

fn parse_positive_i64(name: &str, value: &str) -> io::Result<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|parsed| *parsed > 0)
        .ok_or_else(|| invalid_config(name, value, "a positive integer"))
}

fn parse_positive_u64(name: &str, value: &str) -> io::Result<u64> {
    value
        .parse::<u64>()
        .ok()
        .filter(|parsed| *parsed > 0)
        .ok_or_else(|| invalid_config(name, value, "a positive integer"))
}

fn parse_usize(name: &str, value: &str) -> io::Result<usize> {
    value
        .parse::<usize>()
        .map_err(|_| invalid_config(name, value, "a non-negative integer"))
}

fn parse_stream(name: &str, value: String) -> io::Result<String> {
    if value.trim().is_empty() {
        Err(invalid_config(name, &value, "a non-empty Redis stream key"))
    } else {
        Ok(value)
    }
}

fn invalid_config(name: &str, value: &str, expected: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{name} must be {expected}; got {value:?}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    fn row(payload: &str) -> OutboxEventRow {
        OutboxEventRow {
            event_id: "01JZ0000000000000000000001".to_string(),
            payload: payload.to_string(),
            seq: 42,
            operation_id: Some("01JZ000000000000000000000X".to_string()),
            pair_role: Some("control_plane".to_string()),
            created_at: Utc::now(),
            published_at: None,
        }
    }

    fn config(values: &[(&str, &str)]) -> io::Result<RelayConfig> {
        let values = values
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect::<HashMap<_, _>>();
        RelayConfig::from_lookup(|name| Ok(values.get(name).cloned()))
    }

    #[test]
    fn audit_relay_entry_contains_only_the_unchanged_payload() {
        let payload = "{ \"event_id\" : \"01JZ0000000000000000000001\", \"x\": [1,2] }\n";
        let entry = to_entry(&row(payload));
        assert_eq!(
            entry.fields,
            vec![("payload".to_string(), payload.to_string())]
        );
    }

    #[test]
    fn audit_relay_retry_reuses_the_same_entry_without_local_metadata() {
        let row = row(r#"{"event_id":"01JZ0000000000000000000001"}"#);
        let first = to_entry(&row);
        let retry = to_entry(&row);
        assert_eq!(retry.fields, first.fields);
        assert_eq!(retry.fields.len(), 1);
        assert!(!retry.fields[0].1.contains("\"seq\""));
    }

    #[test]
    fn audit_relay_config_defaults_to_the_shared_stream_contract() {
        assert_eq!(
            config(&[]).unwrap(),
            RelayConfig {
                stream: "audit.raw".to_string(),
                maxlen: Some(100_000),
                batch: 256,
                idle: Duration::from_millis(1000),
            }
        );
    }

    #[test]
    fn audit_relay_config_uses_the_configured_redis_stream() {
        assert_eq!(
            config(&[("AUDIT_REDIS_STREAM", "audit.configured")])
                .unwrap()
                .stream,
            "audit.configured"
        );
    }

    #[test]
    fn audit_relay_config_validates_batch_interval_stream_and_trimming() {
        for values in [
            vec![("AUDIT_RELAY_BATCH", "0")],
            vec![("AUDIT_RELAY_BATCH", "many")],
            vec![("AUDIT_RELAY_INTERVAL_MS", "0")],
            vec![("AUDIT_RELAY_INTERVAL_MS", "soon")],
            vec![("AUDIT_REDIS_STREAM", "  ")],
            vec![("AUDIT_STREAM_MAXLEN", "unbounded")],
        ] {
            assert!(config(&values).is_err(), "accepted {values:?}");
        }
        assert_eq!(
            config(&[("AUDIT_STREAM_MAXLEN", "0")]).unwrap().maxlen,
            None
        );
    }

    #[tokio::test]
    #[ignore = "requires dedicated AUDIT_RELAY_TEST_DATABASE_URL and AUDIT_RELAY_TEST_REDIS_URL services"]
    async fn audit_relay_live_redis_contract_failure_and_duplicate_retry() -> anyhow::Result<()> {
        use db::DbStore;
        use db::ent::{
            CredentialType, Profile, TrustedAdminActor, TrustedAuditContext, TrustedAuditRequest,
        };

        let database_url = std::env::var("AUDIT_RELAY_TEST_DATABASE_URL")
            .expect("AUDIT_RELAY_TEST_DATABASE_URL must name a dedicated PostgreSQL database");
        let redis_url = std::env::var("AUDIT_RELAY_TEST_REDIS_URL")
            .expect("AUDIT_RELAY_TEST_REDIS_URL must name a dedicated Redis service");
        let svc = db::service::init(&database_url)
            .await
            .expect("initialize relay test database");
        let store = RedisStore::new(&redis_url)
            .await
            .expect("initialize relay test Redis store");
        let suffix = ulid::Ulid::new().to_string().to_ascii_lowercase();
        let stream = format!("audit.raw.test.{suffix}");
        let failure_stream = format!("audit.raw.test.failure.{suffix}");

        let client = redis::Client::open(redis_url.as_str()).expect("open Redis test client");
        let mut redis = client
            .get_multiplexed_async_connection()
            .await
            .expect("connect Redis test client");
        let _: usize = redis::cmd("DEL")
            .arg(&stream)
            .arg(&failure_stream)
            .query_async(&mut redis)
            .await
            .expect("clear relay test keys");

        async fn create_event(svc: &Service, tag: &str, request_id: &str) -> anyhow::Result<()> {
            let context = TrustedAuditContext::admin_control_plane(
                TrustedAdminActor::admin("01JZ000000000000000000000A"),
                TrustedAuditRequest::from_http(request_id.to_string(), None, None, None),
            );
            svc.create_user(
                Profile::new(format!("{tag}@example.com"), tag.to_string()),
                CredentialType::Password,
                "AUDIT_RELAY_LIVE_TEST_PASSWORD_HASH",
                context,
            )
            .await?;
            Ok(())
        }

        fn has_request_id(row: &OutboxEventRow, request_id: &str) -> bool {
            serde_json::from_str::<serde_json::Value>(&row.payload)
                .ok()
                .is_some_and(|payload| {
                    payload["request"]["request_id"].as_str() == Some(request_id)
                })
        }

        async fn expected_row(svc: &Service, request_id: &str) -> OutboxEventRow {
            let claim = svc
                .claim_unpublished_outbox_events(100_000)
                .await
                .expect("claim relay test event");
            let row = claim
                .rows()
                .iter()
                .find(|row| has_request_id(row, request_id))
                .cloned()
                .expect("find relay test event in claim");
            drop(claim);
            row
        }

        async fn xrange(
            connection: &mut redis::aio::MultiplexedConnection,
            stream: &str,
        ) -> Vec<(String, Vec<(String, String)>)> {
            redis::cmd("XRANGE")
                .arg(stream)
                .arg("-")
                .arg("+")
                .query_async(connection)
                .await
                .expect("read relay test stream")
        }

        let success_request = ulid::Ulid::new().to_string();
        create_event(&svc, &format!("relay-success-{suffix}"), &success_request)
            .await
            .expect("create success event");
        let success_row = expected_row(&svc, &success_request).await;
        assert!(relay_once(&svc, &store, &stream, None, 100_000).await? > 0);
        let success_entries = xrange(&mut redis, &stream).await;
        assert!(success_entries.iter().any(|(_, fields)| {
            fields.len() == 1 && fields[0].0 == "payload" && fields[0].1 == success_row.payload
        }));

        let retry_request = ulid::Ulid::new().to_string();
        create_event(&svc, &format!("relay-retry-{suffix}"), &retry_request)
            .await
            .expect("create retry event");
        let first_claim = svc
            .claim_unpublished_outbox_events(100_000)
            .await
            .expect("claim first retry attempt");
        let retry_row = first_claim
            .rows()
            .iter()
            .find(|row| has_request_id(row, &retry_request))
            .cloned()
            .expect("find retry event");
        let first_entries = first_claim.rows().iter().map(to_entry).collect::<Vec<_>>();
        store
            .xadd_batch(&stream, None, &first_entries)
            .await
            .expect("publish first retry attempt");
        drop(first_claim);

        let retry_claim = svc
            .claim_unpublished_outbox_events(100_000)
            .await
            .expect("claim duplicate retry attempt");
        let retried_row = retry_claim
            .rows()
            .iter()
            .find(|row| row.event_id == retry_row.event_id)
            .expect("retry must reclaim the same event");
        assert_eq!(retried_row.payload, retry_row.payload);
        let retry_entries = retry_claim.rows().iter().map(to_entry).collect::<Vec<_>>();
        store
            .xadd_batch(&stream, None, &retry_entries)
            .await
            .expect("publish duplicate retry attempt");
        retry_claim
            .commit_published()
            .await
            .expect("mark successful retry published");
        let entries = xrange(&mut redis, &stream).await;
        assert_eq!(
            entries
                .iter()
                .filter(|(_, fields)| {
                    fields.len() == 1
                        && fields[0].0 == "payload"
                        && fields[0].1 == retry_row.payload
                })
                .count(),
            2
        );

        let failure_request = ulid::Ulid::new().to_string();
        create_event(&svc, &format!("relay-failure-{suffix}"), &failure_request)
            .await
            .expect("create failure event");
        let failure_row = expected_row(&svc, &failure_request).await;
        let failure_claim = svc
            .claim_unpublished_outbox_events(100_000)
            .await
            .expect("claim failure event");
        let failure_entries = failure_claim
            .rows()
            .iter()
            .map(to_entry)
            .collect::<Vec<_>>();
        let _: () = redis::cmd("SET")
            .arg(&failure_stream)
            .arg("wrong-type")
            .query_async(&mut redis)
            .await
            .expect("create wrong-type Redis destination");
        assert!(
            store
                .xadd_batch(&failure_stream, None, &failure_entries)
                .await
                .is_err(),
            "Redis WRONGTYPE must fail the handoff"
        );
        drop(failure_claim);
        let pending_claim = svc
            .claim_unpublished_outbox_events(100_000)
            .await
            .expect("reclaim event after Redis failure");
        assert!(
            pending_claim
                .rows()
                .iter()
                .any(|row| row.event_id == failure_row.event_id),
            "Redis failure must leave the row pending"
        );
        drop(pending_claim);

        let _: usize = redis::cmd("DEL")
            .arg(&stream)
            .arg(&failure_stream)
            .query_async(&mut redis)
            .await
            .expect("remove relay test keys");
        Ok(())
    }
}
