//! Durable quota balance.
//!
//! A reservation charges a batch of units to the window before the replica
//! spends them. The conditional upsert is the whole concurrency control: at
//! most `limit` units are ever reserved for one bucket and window, whatever
//! the number of replicas, and a crash only strands the batch it had not
//! consumed.

use crate::backend::Pool;
use anyhow::{Context, Result, ensure};

/// Rows one prune statement deletes at most; callers repeat while a batch is
/// full.
pub const QUOTA_PRUNE_BATCH: u64 = 1000;

#[derive(Clone, Copy)]
pub struct QuotaRepository;

/// Who a durable balance belongs to. Balances of deleted owners are pruned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaOwner<'a> {
    /// A user or an API key.
    Subject(&'a str),
    Organization(&'a str),
}

impl<'a> QuotaOwner<'a> {
    fn kind(self) -> &'static str {
        match self {
            Self::Subject(_) => "subject",
            Self::Organization(_) => "organization",
        }
    }

    fn id(self) -> &'a str {
        match self {
            Self::Subject(id) | Self::Organization(id) => id,
        }
    }
}

/// One atomic reservation request.
pub struct QuotaReservation<'a> {
    pub owner: QuotaOwner<'a>,
    pub bucket: &'a str,
    /// Configured period, time zone and anchor; changing it starts a fresh
    /// balance.
    pub definition: &'a str,
    pub window_start: i64,
    pub window_end: i64,
    pub limit: u64,
    pub units: u64,
}

/// Outcome of a reservation, with the balance in use that decided it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaReserved {
    /// Charged; the balance in use afterwards.
    Granted(u64),
    /// Not charged; the balance that refused it. A stored window newer than
    /// the requested one refuses the whole requested window and reports
    /// the limit.
    Refused(u64),
}

impl QuotaRepository {
    pub fn new() -> Self {
        Self
    }

    /// Reserve `units` in `bucket` for `[window_start, window_end)` with one
    /// statement.
    ///
    /// A row of an earlier window is replaced instead of accumulated, so the
    /// first request of a new window starts from zero. A reservation for a
    /// window older than the stored one with the same definition is refused:
    /// a replica with a stale clock must not roll the balance back.
    /// `definition` identifies the configured period, time zone and anchor;
    /// changing it also starts a fresh balance.
    pub async fn reserve(
        &self,
        pool: &Pool,
        reservation: QuotaReservation<'_>,
    ) -> Result<QuotaReserved> {
        let QuotaReservation {
            owner,
            bucket,
            definition,
            window_start,
            window_end,
            limit,
            units,
        } = reservation;
        // The insert has no condition: a fresh balance must fit on its own.
        ensure!(units <= limit, "quota reservation exceeds its limit");
        let stored_limit = i64::try_from(limit).context("quota limit exceeds i64")?;
        let units = i64::try_from(units).context("quota reservation exceeds i64")?;
        let used: Option<i64> = sqlx::query_scalar(
            "INSERT INTO quota_usage AS q
                 (bucket, owner_kind, owner_id, definition, window_start, window_end, used)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (bucket) DO UPDATE SET
                 used = CASE
                     WHEN q.definition = excluded.definition
                         AND q.window_start = excluded.window_start
                     THEN q.used + excluded.used
                     ELSE excluded.used
                 END,
                 definition = excluded.definition,
                 window_start = excluded.window_start,
                 window_end = excluded.window_end
             WHERE q.definition <> excluded.definition
                 OR q.window_start < excluded.window_start
                 OR (q.window_start = excluded.window_start AND q.used + excluded.used <= $8)
             RETURNING used",
        )
        .bind(bucket)
        .bind(owner.kind())
        .bind(owner.id())
        .bind(definition)
        .bind(window_start)
        .bind(window_end)
        .bind(units)
        .bind(stored_limit)
        .fetch_optional(pool)
        .await?;
        if let Some(used) = used {
            return Ok(QuotaReserved::Granted(
                u64::try_from(used).context("negative quota balance")?,
            ));
        }

        // Read once, only on a refusal: the tracker remembers the balance and
        // denies in memory instead of asking again.
        let stored: Option<(String, i64, i64)> = sqlx::query_as(
            "SELECT definition, window_start, used FROM quota_usage WHERE bucket = $1",
        )
        .bind(bucket)
        .fetch_optional(pool)
        .await?;
        let used = match stored {
            Some((stored, start, used)) if stored == definition && start == window_start => {
                u64::try_from(used).context("negative quota balance")?
            }
            Some((stored, start, _)) if stored == definition && start > window_start => limit,
            // The row changed since the refusal: the next reservation decides.
            _ => 0,
        };
        Ok(QuotaReserved::Refused(used))
    }

    /// Delete up to [`QUOTA_PRUNE_BATCH`] balances whose subject or
    /// organization no longer exists. Returns the number of rows deleted.
    pub async fn prune_orphaned(&self, pool: &Pool) -> Result<u64> {
        let statement = format!(
            "DELETE FROM quota_usage WHERE bucket IN (
                 SELECT q.bucket FROM quota_usage q
                 WHERE (q.owner_kind = 'organization' AND NOT EXISTS (
                         SELECT 1 FROM organizations o WHERE o.id = q.owner_id))
                     OR (q.owner_kind = 'subject'
                         AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id = q.owner_id)
                         AND NOT EXISTS (SELECT 1 FROM api_keys k WHERE k.id = q.owner_id))
                 LIMIT {QUOTA_PRUNE_BATCH} {skip_locked}
             )",
            skip_locked = crate::backend::FOR_UPDATE_SKIP_LOCKED,
        );
        Ok(sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(pool)
            .await?
            .rows_affected())
    }
}

impl Default for QuotaRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test_support {
    use super::{QuotaOwner, QuotaRepository, QuotaReservation, QuotaReserved, Result};
    use crate::backend::Pool;

    pub(super) const OWNER: QuotaOwner<'static> = QuotaOwner::Organization("quota-tests");

    pub(super) fn bucket() -> String {
        format!("stargate:v1:{{sub:{}}}:quota:test", ulid::Ulid::generate())
    }

    pub(super) async fn create_owner(pool: &Pool) {
        sqlx::query(
            "INSERT INTO organizations (id, name) VALUES ($1, 'Quota tests')
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(OWNER.id())
        .execute(pool)
        .await
        .unwrap();
    }

    pub(super) async fn reserve(
        pool: &Pool,
        bucket: &str,
        definition: &str,
        window_start: i64,
        window_end: i64,
        limit: u64,
        units: u64,
    ) -> Result<QuotaReserved> {
        QuotaRepository::new()
            .reserve(
                pool,
                QuotaReservation {
                    owner: OWNER,
                    bucket,
                    definition,
                    window_start,
                    window_end,
                    limit,
                    units,
                },
            )
            .await
    }

    /// Reserve one unit for `owner`, then delete the owners in `deleted`
    /// and prune. Returns which of `owners` kept their balance.
    pub(super) async fn prune_after_deleting(pool: &Pool) -> [bool; 5] {
        let suffix = ulid::Ulid::generate().to_string();
        let user = format!("user-{suffix}");
        let api_key = format!("key-{suffix}");
        let organization = format!("org-{suffix}");
        let deleted_user = format!("gone-user-{suffix}");
        let deleted_organization = format!("gone-org-{suffix}");
        for id in [&user, &deleted_user] {
            sqlx::query("INSERT INTO users (id, email, nickname) VALUES ($1, $2, $1)")
                .bind(id)
                .bind(format!("{id}@example.test"))
                .execute(pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO api_keys (id, key_hash, label) VALUES ($1, $1, 'quota')")
            .bind(&api_key)
            .execute(pool)
            .await
            .unwrap();
        for id in [&organization, &deleted_organization] {
            sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'Quota')")
                .bind(id)
                .execute(pool)
                .await
                .unwrap();
        }
        let owners = [
            QuotaOwner::Subject(&user),
            QuotaOwner::Subject(&api_key),
            QuotaOwner::Organization(&organization),
            QuotaOwner::Subject(&deleted_user),
            QuotaOwner::Organization(&deleted_organization),
        ];
        let buckets = owners.map(|_| bucket());
        for (owner, bucket) in owners.iter().zip(&buckets) {
            QuotaRepository::new()
                .reserve(
                    pool,
                    QuotaReservation {
                        owner: *owner,
                        bucket,
                        definition: "day:UTC",
                        window_start: 100,
                        window_end: 200,
                        limit: 10,
                        units: 1,
                    },
                )
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(&deleted_user)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(&deleted_organization)
            .execute(pool)
            .await
            .unwrap();

        while QuotaRepository::new().prune_orphaned(pool).await.unwrap() == super::QUOTA_PRUNE_BATCH
        {
        }

        let mut kept = [false; 5];
        for (kept, bucket) in kept.iter_mut().zip(&buckets) {
            *kept =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM quota_usage WHERE bucket = $1")
                    .bind(bucket)
                    .fetch_one(pool)
                    .await
                    .unwrap()
                    == 1;
        }
        kept
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod sqlite_tests {
    use super::QuotaReserved::{Granted, Refused};
    use super::test_support::{self, bucket, reserve};
    use crate::backend::Pool;
    use std::path::PathBuf;

    struct TestDatabase {
        path: PathBuf,
        pool: Pool,
    }

    impl TestDatabase {
        async fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("stargate-quota-{}.db", ulid::Ulid::generate()));
            let pool = Pool::connect(&format!("sqlite://{}?mode=rwc", path.display()))
                .await
                .unwrap();
            crate::service::Service::new(pool.clone())
                .migrate()
                .await
                .unwrap();
            test_support::create_owner(&pool).await;
            Self { path, pool }
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[tokio::test]
    async fn reservations_charge_batches_and_never_exceed_the_limit() {
        let database = TestDatabase::new().await;
        let pool = &database.pool;
        let bucket = bucket();
        assert_eq!(
            reserve(pool, &bucket, "day:UTC", 100, 200, 10, 4)
                .await
                .unwrap(),
            Granted(4)
        );
        assert_eq!(
            reserve(pool, &bucket, "day:UTC", 100, 200, 10, 7)
                .await
                .unwrap(),
            Refused(4)
        );
        assert_eq!(
            reserve(pool, &bucket, "day:UTC", 100, 200, 10, 6)
                .await
                .unwrap(),
            Granted(10)
        );
        assert!(
            reserve(pool, &bucket, "day:UTC", 100, 200, 10, 11)
                .await
                .is_err(),
            "a batch larger than the limit is a caller error"
        );
    }

    #[tokio::test]
    async fn a_new_window_replaces_the_old_balance() {
        let database = TestDatabase::new().await;
        let pool = &database.pool;
        let bucket = bucket();
        assert_eq!(
            reserve(pool, &bucket, "day:UTC", 100, 200, 10, 7)
                .await
                .unwrap(),
            Granted(7)
        );
        assert_eq!(
            reserve(pool, &bucket, "day:UTC", 200, 300, 10, 3)
                .await
                .unwrap(),
            Granted(3)
        );
        // A stale replica cannot roll the balance back to the previous
        // window, and learns that its whole window is spent.
        assert_eq!(
            reserve(pool, &bucket, "day:UTC", 100, 200, 10, 1)
                .await
                .unwrap(),
            Refused(10)
        );
        // Changing the window definition starts a fresh balance.
        assert_eq!(
            reserve(pool, &bucket, "month:UTC:15", 200, 300, 10, 4)
                .await
                .unwrap(),
            Granted(4)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_replicas_reserve_exactly_the_limit() {
        let database = TestDatabase::new().await;
        let bucket = bucket();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..32 {
            let pool = database.pool.clone();
            let bucket = bucket.clone();
            tasks.spawn(async move {
                matches!(
                    reserve(&pool, &bucket, "day:UTC", 100, 200, 10, 1)
                        .await
                        .unwrap(),
                    Granted(_)
                )
            });
        }
        let mut granted = 0;
        while let Some(result) = tasks.join_next().await {
            granted += usize::from(result.unwrap());
        }
        assert_eq!(granted, 10);
    }

    #[tokio::test]
    async fn pruning_removes_only_the_balances_of_deleted_owners() {
        let database = TestDatabase::new().await;
        assert_eq!(
            test_support::prune_after_deleting(&database.pool).await,
            [true, true, true, false, false]
        );
    }
}

#[cfg(all(test, feature = "postgres"))]
mod postgres_tests {
    use super::QuotaReserved::Granted;
    use super::test_support::{self, bucket, reserve};
    use crate::backend::Pool;

    async fn pool() -> Pool {
        dotenvy::dotenv().ok();
        let url = std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .expect("TEST_DATABASE_URL or DATABASE_URL must be set");
        let pool = Pool::connect(&url).await.unwrap();
        crate::service::Service::new(pool.clone())
            .migrate()
            .await
            .unwrap();
        test_support::create_owner(&pool).await;
        pool
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn postgres_reservations_are_atomic_across_connections() {
        let pool = pool().await;
        let bucket = bucket();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..32 {
            let pool = pool.clone();
            let bucket = bucket.clone();
            tasks.spawn(async move {
                matches!(
                    reserve(&pool, &bucket, "day:UTC", 100, 200, 10, 1)
                        .await
                        .unwrap(),
                    Granted(_)
                )
            });
        }
        let mut granted = 0;
        while let Some(result) = tasks.join_next().await {
            granted += usize::from(result.unwrap());
        }
        assert_eq!(granted, 10);
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn postgres_refusals_report_the_blocking_balance() {
        use super::QuotaReserved::Refused;
        let pool = pool().await;
        let bucket = bucket();
        assert_eq!(
            reserve(&pool, &bucket, "day:UTC", 200, 300, 10, 7)
                .await
                .unwrap(),
            Granted(7)
        );
        assert_eq!(
            reserve(&pool, &bucket, "day:UTC", 200, 300, 10, 4)
                .await
                .unwrap(),
            Refused(7)
        );
        assert_eq!(
            reserve(&pool, &bucket, "day:UTC", 100, 200, 10, 1)
                .await
                .unwrap(),
            Refused(10)
        );
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn postgres_pruning_removes_only_the_balances_of_deleted_owners() {
        let pool = pool().await;
        assert_eq!(
            test_support::prune_after_deleting(&pool).await,
            [true, true, true, false, false]
        );
    }
}
