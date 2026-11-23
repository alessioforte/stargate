use super::Gate;
use crate::data::db;
use crate::gate::snap::{
    config::GatewayPreparationError,
    reload::{apply_gateway_document, apply_policy_document},
};
use ::db::ent::ConfigKind;
use once_cell::sync::OnceCell;
use std::{io, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, Notify},
    task::JoinHandle,
    time::MissedTickBehavior,
};

const INTERVAL_ENV: &str = "CONFIG_SYNC_INTERVAL_SECS";

fn sync_interval(value: Option<&str>) -> io::Result<Duration> {
    let seconds = value.unwrap_or("30").parse::<u64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{INTERVAL_ENV} must be an integer >= 1"),
        )
    })?;
    let interval = Duration::from_secs(seconds);
    if seconds == 0 || std::time::Instant::now().checked_add(interval).is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{INTERVAL_ENV} must be a positive, representable timer interval"),
        ));
    }
    Ok(interval)
}

pub(crate) fn spawn_config_sync(gate: Arc<Gate>) -> io::Result<()> {
    let value = match std::env::var(INTERVAL_ENV) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(io::Error::other(error)),
    };
    let interval = sync_interval(value.as_deref())?;
    spawn(gate, interval)
}

pub(crate) async fn shutdown_config_sync() {
    shutdown().await;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Trigger {
    #[cfg(any(feature = "postgres", test))]
    Notification,
    Tick,
}

#[derive(Default)]
struct RevisionState {
    failed_version: Option<i64>,
}

impl RevisionState {
    fn should_apply(&self, is_active: bool, desired: i64, trigger: Trigger) -> bool {
        !is_active && (trigger == Trigger::Tick || self.failed_version != Some(desired))
    }

    /// Returns whether this failure needs its first log entry.
    fn failed(&mut self, version: i64) -> bool {
        let first = self.failed_version != Some(version);
        self.failed_version = Some(version);
        first
    }

    fn applied(&mut self) {
        self.failed_version = None;
    }
}

struct SyncHandle {
    stop: Arc<Notify>,
    task: Mutex<Option<JoinHandle<()>>>,
}

static SYNC: OnceCell<SyncHandle> = OnceCell::new();

fn spawn(gate: Arc<Gate>, interval: Duration) -> io::Result<()> {
    if SYNC.get().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "configuration synchronizer already started",
        ));
    }
    let stop = Arc::new(Notify::new());
    let task = tokio::spawn(run(gate, stop.clone(), Wakeups::new(interval)));
    let handle = SyncHandle {
        stop,
        task: Mutex::new(Some(task)),
    };
    if let Err(handle) = SYNC.set(handle) {
        if let Some(task) = handle.task.into_inner() {
            task.abort();
        }
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "configuration synchronizer already started",
        ));
    }
    Ok(())
}

async fn shutdown() {
    if let Some(handle) = SYNC.get() {
        handle.stop.notify_one();
        if let Some(task) = handle.task.lock().await.take() {
            let _ = task.await;
        }
    }
}

async fn run(gate: Arc<Gate>, stop: Arc<Notify>, wakeups: Wakeups) {
    // Cancel pending listener/pool queries as well as idle waits on shutdown.
    tokio::select! {
        biased;
        _ = stop.notified() => {},
        _ = work(&gate, wakeups) => {},
    }
    tracing::info!("Configuration synchronizer stopped");
}

struct Wakeups {
    ticks: tokio::time::Interval,
    #[cfg(feature = "postgres")]
    listener: Option<::db::PgListener>,
}

impl Wakeups {
    fn new(interval: Duration) -> Self {
        let mut ticks = tokio::time::interval(interval);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Self {
            ticks,
            #[cfg(feature = "postgres")]
            listener: None,
        }
    }

    async fn next(&mut self) -> Trigger {
        #[cfg(not(feature = "postgres"))]
        {
            self.ticks.tick().await;
            Trigger::Tick
        }
        #[cfg(feature = "postgres")]
        loop {
            return tokio::select! {
                _ = self.ticks.tick() => {
                    if self.listener.is_none() {
                        match db::config_listener().await {
                            Ok(connected) => self.listener = Some(connected),
                            Err(error) => tracing::error!(%error, "Unable to subscribe to configuration notifications; retrying on the next tick"),
                        }
                    }
                    Trigger::Tick
                },
                result = async { self.listener.as_mut().expect("enabled listener branch").try_recv().await }, if self.listener.is_some() => {
                    match result {
                        // Eager reconnection completed; notifications in the gap were lost.
                        Ok(Some(_)) | Ok(None) => Trigger::Notification,
                        Err(error) => {
                            self.listener = None;
                            tracing::error!(%error, "Configuration listener failed; reconnecting on the next tick");
                            continue;
                        }
                    }
                },
            };
        }
    }
}

async fn work(gate: &Arc<Gate>, mut wakeups: Wakeups) {
    let mut gateway = RevisionState::default();
    let mut policies = RevisionState::default();
    let mut database_failed = false;
    loop {
        let trigger = wakeups.next().await;
        if database_failed && trigger != Trigger::Tick {
            continue;
        }
        match synchronize(gate, &mut gateway, &mut policies, trigger).await {
            Ok(()) => database_failed = false,
            Err(error) => {
                database_failed = true;
                tracing::error!(%error, "Unable to read configuration documents; retrying on the next tick");
            }
        }
    }
}

async fn synchronize(
    gate: &Arc<Gate>,
    gateway: &mut RevisionState,
    policies: &mut RevisionState,
    trigger: Trigger,
) -> anyhow::Result<()> {
    for ::db::ent::ConfigRevision {
        kind,
        version,
        revision,
    } in db::get_config_revisions().await?
    {
        let state = match kind {
            ConfigKind::Gateway => &mut *gateway,
            ConfigKind::Policies => &mut *policies,
        };
        let is_active = |version, revision: &str| match kind {
            ConfigKind::Gateway => gate.snapshot().config_document_version >= version,
            ConfigKind::Policies => gate.policy_snapshot.load().revision == revision,
        };
        if !state.should_apply(is_active(version, &revision), version, trigger) {
            continue;
        }
        let Some(document) = db::get_active_config_document(kind).await? else {
            continue;
        };
        let version = document.version;
        if !state.should_apply(is_active(version, &document.revision), version, trigger) {
            continue;
        }
        let revision = document.revision.clone();
        let result = match kind {
            ConfigKind::Gateway => apply_gateway_document(gate, document)
                .await
                .map(|_| ())
                .map_err(|error| {
                    (
                        matches!(&error, GatewayPreparationError::BudgetChange),
                        error.reload_outcome(),
                    )
                }),
            ConfigKind::Policies => apply_policy_document(gate, &document.content)
                .map(|_| ())
                .map_err(|_| (false, "error")),
        };
        match result {
            Ok(()) => state.applied(),
            Err((restart_required, outcome)) if state.failed(version) => {
                if restart_required {
                    tracing::warn!(kind = kind.as_str(), %revision, "Gateway configuration restart required; keeping the active snapshot");
                } else {
                    tracing::error!(kind = kind.as_str(), %revision, outcome, "Configuration activation failed; keeping the active snapshot and retrying on the next tick");
                }
            }
            Err(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
