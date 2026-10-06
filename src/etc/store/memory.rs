use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};
use store::{MemoryStore, memory::MemoryStoreConfig};

pub const MEMORY_BACKUP_PATH: &str = ".stargate/memory.bin";
const DEFAULT_MEMORY_BACKUP_INTERVAL_SECS: u64 = 30;
const DEFAULT_MEMORY_BACKUP_WRITE_THRESHOLD: u64 = 100;
const MEMORY_BACKUP_CHECK_INTERVAL_SECS: u64 = 1;
const DEFAULT_MAX_MEMORY_MB: usize = 100;

static RESTORE_ATTEMPTED: AtomicBool = AtomicBool::new(false);
static BACKUP_TASK_STARTED: AtomicBool = AtomicBool::new(false);

pub static STORE: Lazy<MemoryStore> = Lazy::new(|| {
    let max_memory_mb: usize = std::env::var("STORE_MAX_MEMORY_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_MEMORY_MB);

    let config = MemoryStoreConfig {
        max_memory_usage: max_memory_mb * 1024 * 1024,
        ..MemoryStoreConfig::default()
    };
    let store = MemoryStore::with_config(config);
    store.run_cleaner(60);
    store
});

pub async fn init() -> &'static MemoryStore {
    Lazy::force(&STORE);
    restore_memory_backup().await;
    tracing::info!("Memory store initialized");
    run_memory_backup();
    &STORE
}

pub fn use_store() -> &'static MemoryStore {
    &STORE
}

/// Restores the in-memory store from the binary snapshot written by a previous process run.
///
/// # When it runs
///
/// Called once during [`init`], before the backup task starts. The [`RESTORE_ATTEMPTED`]
/// flag ensures idempotence: even if `init` is somehow invoked concurrently, only the first
/// caller performs the restore. Subsequent callers return immediately.
///
/// # What it does
///
/// 1. Loads the MessagePack snapshot from [`MEMORY_BACKUP_PATH`] (`.stargate/memory.bin`).
/// 2. Replaces the contents of the already-initialised global [`STORE`] (empty at this
///    point since the `Lazy` constructor does not populate any data) via
///    `MemoryStore::absorb`, which also resets operational stats to zero so metrics
///    reflect the current process lifetime, not the previous one.
///
/// # Failure handling
///
/// A missing or corrupt snapshot is treated as a normal cold start — the error is logged
/// at `INFO` level and the empty store is used. The process never panics here; losing
/// the snapshot is recoverable (state will rebuild from application traffic).
async fn restore_memory_backup() {
    if RESTORE_ATTEMPTED.swap(true, Ordering::AcqRel) {
        return;
    }

    match MemoryStore::load_from_binary(MEMORY_BACKUP_PATH).await {
        Ok(restored_store) => {
            STORE.absorb(&restored_store);
            tracing::info!(
                path = MEMORY_BACKUP_PATH,
                keys = STORE.get_total_keys(),
                "Memory store restored from backup"
            );
        }
        Err(error) => {
            tracing::info!(
                path = MEMORY_BACKUP_PATH,
                %error,
                "No memory backup restored at startup"
            );
        }
    }
}

/// Spawns the background task that periodically persists the memory store to disk.
///
/// # Idempotence
///
/// [`BACKUP_TASK_STARTED`] is a one-shot flag. Only the first call spawns a task;
/// any subsequent call (e.g. from a test harness calling `init` twice) is a no-op.
///
/// # Configuration (env vars)
///
/// | Variable                     | Default | Meaning                                                    |
/// |------------------------------|---------|------------------------------------------------------------|
/// | `STORE_BACKUP_INTERVAL_SECS` | `30`    | Maximum seconds between two consecutive writes             |
/// | `STORE_BACKUP_WRITE_THRESHOLD` | `100` | Minimum mutations accumulated before an early write fires; set to `0` to disable threshold-based flushing |
///
/// # How the flush decision works
///
/// The task wakes up every second ([`MEMORY_BACKUP_CHECK_INTERVAL_SECS`]) with
/// `MissedTickBehavior::Skip` so a slow disk write never queues up extra ticks.
///
/// On each wake-up the task reads [`MemoryStore::persistence_revision`] — a monotonically
/// increasing counter that advances with every mutation (set, delete, hash-set, hash-delete,
/// and expired-entry cleanup). If the revision has not changed since the last successful
/// write the store is clean and the task sleeps again.
///
/// When the revision has advanced, a write is triggered if **either** condition is true:
///
/// - **Time condition** — at least `STORE_BACKUP_INTERVAL_SECS` have elapsed since the
///   last successful write. Guarantees a worst-case data-loss window regardless of write rate.
/// - **Threshold condition** — the number of mutations accumulated since the last write has
///   reached `STORE_BACKUP_WRITE_THRESHOLD`. Triggers an early flush under write bursts so a
///   sudden crash does not lose a large batch of work.
///
/// Both conditions must be false for the write to be skipped. If either fires the store is
/// serialised to [`MEMORY_BACKUP_PATH`] as a MessagePack binary via an atomic temp-file +
/// rename operation (performed inside `save_to_binary`).
///
/// # Failure handling
///
/// A failed write is logged at `WARN` level. `last_persisted_revision` and
/// `last_persisted_at` are **not** updated on failure, so the next check will retry
/// immediately on the next tick that satisfies the flush conditions.
///
/// # Shutdown
///
/// This task runs indefinitely. A final synchronous flush is performed by [`save`] during
/// graceful shutdown to minimise data loss between the last periodic write and process exit.
fn run_memory_backup() {
    if BACKUP_TASK_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    let interval_secs = std::env::var("STORE_BACKUP_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v: &u64| *v > 0)
        .unwrap_or(DEFAULT_MEMORY_BACKUP_INTERVAL_SECS);
    let interval = std::time::Duration::from_secs(interval_secs);
    let write_threshold = std::env::var("STORE_BACKUP_WRITE_THRESHOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(|v: u64| if v == 0 { None } else { Some(v) })
        .unwrap_or(Some(DEFAULT_MEMORY_BACKUP_WRITE_THRESHOLD));
    let check_interval = std::time::Duration::from_secs(MEMORY_BACKUP_CHECK_INTERVAL_SECS);
    tokio::spawn(async move {
        let mut last_persisted_revision = use_store().persistence_revision();
        let mut last_persisted_at = tokio::time::Instant::now();
        let mut ticker = tokio::time::interval(check_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tracing::info!(
            interval_secs,
            write_threshold,
            "Memory store backup task started"
        );
        loop {
            ticker.tick().await;

            let store = use_store();
            let current_revision = store.persistence_revision();
            if current_revision == last_persisted_revision {
                continue;
            }

            let writes_since_last_persist =
                current_revision.saturating_sub(last_persisted_revision);
            let interval_elapsed = last_persisted_at.elapsed() >= interval;
            let threshold_reached =
                write_threshold.is_some_and(|threshold| writes_since_last_persist >= threshold);

            if !interval_elapsed && !threshold_reached {
                continue;
            }

            if let Err(error) = store.save_to_binary(MEMORY_BACKUP_PATH).await {
                tracing::warn!(
                    path = MEMORY_BACKUP_PATH,
                    %error,
                    "Failed to persist memory backup"
                );
            } else {
                last_persisted_revision = current_revision;
                last_persisted_at = tokio::time::Instant::now();
            }
        }
    });
}

pub async fn save() {
    let store = use_store();
    if let Err(error) = store.save_to_binary(MEMORY_BACKUP_PATH).await {
        tracing::warn!("Failed to save memory store on shutdown: {}", error);
    } else {
        tracing::info!("Memory store saved on shutdown");
    }
}
