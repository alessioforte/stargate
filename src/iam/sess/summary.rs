//! Estimated session totals for the admin overview.
//!
//! Opening or ending a session never updates a counter. A reconciliation walks
//! the users in bounded batches, counts the sessions in their indexes that pass
//! the checks of `load_active_session`, and publishes the totals with the time
//! the walk completed. The overview reads only that record.
//!
//! Totals change only when a walk completes. A new, expired or revoked session,
//! or one whose subject became unreadable, is reflected by the next walk: the
//! published totals lag behind by about one interval and one tick plus up to
//! two walks, and a failed walk leaves the previous totals and their older time
//! in place. Replicas share the totals and take turns through a lease: the
//! walking replica renews it after each batch and after publishing, so one walk
//! runs at a time and the next starts an interval after it ends.

use super::listing::{CONCURRENCY, indexed_sids};
use super::registry::{get_active_session, now_unix};
use crate::data::store::{use_store, without_security_budget};
use futures_util::future::try_join_all;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use store::{Store, StoreError};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

/// Time from the end of a walk to the start of the next, across replicas: the
/// lease's lifetime, renewed while a walk runs.
const INTERVAL: Duration = Duration::from_secs(60);
/// How often a replica checks whether a walk is due.
const TICK: Duration = Duration::from_secs(10);
/// Users counted per batch.
const BATCH: i64 = 500;
/// Pause between batches, which spreads a walk's load on the store.
const PAUSE: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub active_sessions: u64,
    pub active_users: u64,
    /// Unix seconds when the walk that produced these totals completed.
    pub reconciled_at_unix: u64,
}

/// The totals of the last completed walk, or `None` before the first one.
pub async fn summary() -> Result<Option<SessionSummary>, StoreError> {
    match use_store().get(&store::keys::session::summary()).await {
        // A record in an older format is replaced by the next walk.
        Err(StoreError::DeserializationFailed(_)) => Ok(None),
        result => result,
    }
}

/// Walk every user once and publish the totals.
pub async fn reconcile() -> anyhow::Result<SessionSummary> {
    without_security_budget(async {
        let mut walk = Walk::default();
        while let Some(batch) = count_batch(walk.after.clone()).await? {
            walk.apply(batch);
            renew_lease().await;
            tokio::time::sleep(PAUSE).await;
        }
        let summary = walk.finish(now_unix());
        use_store()
            .set(&store::keys::session::summary(), &summary, None)
            .await?;
        renew_lease().await;
        Ok(summary)
    })
    .await
}

/// Keep the lease for another interval, so no replica starts a walk while this
/// one runs. Without a lease there is nothing to renew.
async fn renew_lease() {
    if let Err(error) = use_store()
        .expire(&store::keys::session::reconciliation(), INTERVAL.as_secs())
        .await
    {
        tracing::debug!(%error, "Session reconciliation lease not renewed");
    }
}

/// The progress of one walk: the last user counted and the running totals.
#[derive(Debug, Default)]
struct Walk {
    after: Option<String>,
    sessions: u64,
    users: u64,
}

/// The users after `after`, through `last`, and their live sessions.
#[derive(Debug, Clone)]
struct Batch {
    after: Option<String>,
    last: String,
    sessions: u64,
    users: u64,
}

impl Walk {
    /// Count a batch once. A batch that does not continue where the walk
    /// stands, such as a repeated one, changes nothing.
    fn apply(&mut self, batch: Batch) -> bool {
        if batch.after != self.after {
            return false;
        }
        self.sessions += batch.sessions;
        self.users += batch.users;
        self.after = Some(batch.last);
        true
    }

    fn finish(self, now: u64) -> SessionSummary {
        SessionSummary {
            active_sessions: self.sessions,
            active_users: self.users,
            reconciled_at_unix: now,
        }
    }
}

async fn count_batch(after: Option<String>) -> anyhow::Result<Option<Batch>> {
    let users = crate::data::db::service()
        .users_before(None, after.as_deref(), BATCH)
        .await?;
    let Some(last) = users.last().map(|user| user.id.clone()) else {
        return Ok(None);
    };
    let ids: Vec<&str> = users.iter().map(|user| user.id.as_str()).collect();
    let (sessions, users) = count_live(&ids).await?;
    Ok(Some(Batch {
        after,
        last,
        sessions,
        users,
    }))
}

/// Live sessions of these users, and how many of the users hold one.
async fn count_live(user_ids: &[&str]) -> Result<(u64, u64), StoreError> {
    let mut candidates = Vec::new();
    for users in user_ids.chunks(CONCURRENCY) {
        let indexes = try_join_all(users.iter().map(|user_id| indexed_sids(user_id))).await?;
        for (user_id, sids) in users.iter().zip(indexes) {
            candidates.extend(sids.into_iter().map(|sid| (*user_id, sid)));
        }
    }
    let mut owners = HashSet::new();
    let mut sessions = 0;
    for chunk in candidates.chunks(CONCURRENCY) {
        let checks = chunk
            .iter()
            .map(|(user_id, sid)| get_active_session(sid, user_id));
        for ((user_id, _), session) in chunk.iter().zip(try_join_all(checks).await?) {
            if session.is_some() {
                sessions += 1;
                owners.insert(*user_id);
            }
        }
    }
    Ok((sessions, owners.len() as u64))
}

struct Reconciler {
    stop: Arc<Notify>,
    task: Mutex<Option<JoinHandle<()>>>,
}

static RECONCILER: OnceCell<Reconciler> = OnceCell::new();

/// Start reconciling the totals in the background.
pub fn spawn() -> io::Result<()> {
    let already_started = || {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "session reconciliation already started",
        )
    };
    if RECONCILER.get().is_some() {
        return Err(already_started());
    }
    let stop = Arc::new(Notify::new());
    let task = tokio::spawn(run(stop.clone()));
    let reconciler = Reconciler {
        stop,
        task: Mutex::new(Some(task)),
    };
    if let Err(reconciler) = RECONCILER.set(reconciler) {
        if let Some(task) = reconciler.task.into_inner() {
            task.abort();
        }
        return Err(already_started());
    }
    Ok(())
}

/// Stop reconciling, cancelling a walk in progress.
pub async fn shutdown() {
    if let Some(reconciler) = RECONCILER.get() {
        reconciler.stop.notify_one();
        if let Some(task) = reconciler.task.lock().await.take() {
            let _ = task.await;
        }
    }
}

async fn run(stop: Arc<Notify>) {
    let mut ticks = tokio::time::interval(TICK);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tokio::select! {
        biased;
        _ = stop.notified() => {},
        _ = async {
            loop {
                ticks.tick().await;
                reconcile_when_due().await;
            }
        } => {},
    }
    tracing::info!("Session reconciliation stopped");
}

/// Walk when this replica takes the lease. It expires an interval after the
/// walk's last renewal.
async fn reconcile_when_due() {
    let lease = use_store()
        .set_if_absent(
            &store::keys::session::reconciliation(),
            &now_unix(),
            Some(INTERVAL.as_secs()),
        )
        .await;
    match lease {
        Ok(true) => match reconcile().await {
            Ok(summary) => tracing::debug!(?summary, "Session totals reconciled"),
            Err(error) => tracing::warn!(%error, "Session reconciliation failed"),
        },
        Ok(false) => {}
        // The store reports its own outages; the totals' age shows the delay.
        Err(error) => tracing::debug!(%error, "Session reconciliation lease unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(after: Option<&str>, last: &str, sessions: u64, users: u64) -> Batch {
        Batch {
            after: after.map(str::to_string),
            last: last.to_string(),
            sessions,
            users,
        }
    }

    #[test]
    fn a_repeated_batch_counts_once() {
        let mut walk = Walk::default();
        let first = batch(None, "u9", 4, 2);
        assert!(walk.apply(first.clone()));
        assert!(!walk.apply(first));
        assert!(!walk.apply(batch(Some("u5"), "u1", 7, 7)));
        assert!(walk.apply(batch(Some("u9"), "u5", 1, 1)));
        assert_eq!(
            walk.finish(42),
            SessionSummary {
                active_sessions: 5,
                active_users: 3,
                reconciled_at_unix: 42,
            }
        );
    }

    /// The totals live in one process-wide key, so this test owns a process.
    #[cfg(feature = "sqlite")]
    #[test]
    fn reconciled_totals_follow_live_sessions_with_their_freshness() {
        use crate::data::db::with_test_database;
        use crate::iam::sess::test_support::{
            corrupt_subject, create_user, expire_record, new_sid, seed_session,
        };

        const CHILD: &str = "STARGATE_SESSION_SUMMARY_TEST";
        if std::env::var_os(CHILD).is_none() {
            let output = crate::conf::child_test::run(
                "iam::sess::summary::tests::reconciled_totals_follow_live_sessions_with_their_freshness",
                |command| {
                    command.env(CHILD, "1");
                },
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(with_test_database(async {
            let [alice, bob, carol, _idle] = [
                create_user().await,
                create_user().await,
                create_user().await,
                create_user().await,
            ];
            let [a_live, a_expired, a_unreadable, b_unreadable, c1, c2] =
                [(); 6].map(|_| new_sid());
            for (user, sid) in [
                (&alice, &a_live),
                (&alice, &a_expired),
                (&alice, &a_unreadable),
                (&bob, &b_unreadable),
                (&carol, &c1),
                (&carol, &c2),
            ] {
                seed_session(&user.id, sid, None).await;
            }
            expire_record(&a_expired).await;
            corrupt_subject(&a_unreadable).await;
            corrupt_subject(&b_unreadable).await;

            let totals = |sessions, users| (sessions, users);
            let observed =
                |summary: SessionSummary| (summary.active_sessions, summary.active_users);
            let started = now_unix();
            let first = reconcile().await.unwrap();
            assert_eq!(observed(first), totals(3, 2));
            assert!((started..=now_unix()).contains(&first.reconciled_at_unix));
            assert_eq!(summary().await.unwrap(), Some(first));
            // Walking again counts every session once.
            assert_eq!(observed(reconcile().await.unwrap()), totals(3, 2));

            // Totals move with the next walk; Carol keeps her other session.
            corrupt_subject(&c1).await;
            assert_eq!(summary().await.unwrap().map(observed), Some(totals(3, 2)));
            assert_eq!(observed(reconcile().await.unwrap()), totals(2, 2));
            // A walk keeps the lease for an interval after it ends.
            let lease = store::keys::session::reconciliation();
            assert!(
                use_store()
                    .set_if_absent(&lease, &now_unix(), Some(1))
                    .await
                    .unwrap()
            );
            expire_record(&a_live).await;
            assert_eq!(observed(reconcile().await.unwrap()), totals(1, 1));
            tokio::time::sleep(Duration::from_millis(1_500)).await;
            assert!(use_store().exists(&lease).await.unwrap());
        }));
    }
}
