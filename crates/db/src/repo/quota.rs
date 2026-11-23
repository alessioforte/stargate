//! Durable quota balance.
//!
//! A reservation charges a batch of units to the window before the replica
//! spends them. The conditional upsert is the whole concurrency control: at
//! most `limit` units are ever reserved for one bucket and window, whatever
//! the number of replicas, and a crash only strands the batch it had not
//! consumed.

use crate::backend::Pool;
use anyhow::{Context, Result, ensure};
use std::sync::LazyLock;

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
    pub config_version: i64,
    pub now: i64,
    pub window_start: i64,
    pub window_end: i64,
    pub limit: u64,
    pub units: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaBalance {
    pub window_start: i64,
    pub window_end: i64,
    pub used: u64,
}

impl QuotaBalance {
    fn from_row((window_start, window_end, used): (i64, i64, i64)) -> Result<Self> {
        Ok(Self {
            window_start,
            window_end,
            used: u64::try_from(used).context("negative quota balance")?,
        })
    }
}

/// Outcome of a reservation, with the balance in use that decided it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaReserved {
    Granted(QuotaBalance),
    Refused(QuotaBalance),
    StaleConfig,
}

#[cfg(feature = "postgres")]
const CONFIG_SHARE_LOCK: &str = "FOR SHARE";
#[cfg(feature = "sqlite")]
const CONFIG_SHARE_LOCK: &str = "";

// Hold the PostgreSQL row lock through the upsert so an old reservation
// either completes before the config save or sees the new version and stops.
// SQLite serializes these statements as writes.
static RESERVE_SQL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "INSERT INTO quota_usage AS q (bucket, owner_kind, owner_id, window_start, window_end, used)
         SELECT $1, $2, $3, $4, $5, $6 FROM quota_config
         WHERE id = 1 AND version = $9 {CONFIG_SHARE_LOCK}
         ON CONFLICT (bucket) DO UPDATE SET
             used = CASE WHEN $7 < q.window_end THEN q.used + excluded.used ELSE excluded.used END,
             window_start = CASE
                 WHEN $7 < q.window_end THEN q.window_start
                 WHEN excluded.window_start < q.window_end THEN q.window_end
                 ELSE excluded.window_start
             END,
             window_end = CASE WHEN $7 < q.window_end THEN q.window_end ELSE excluded.window_end END
         WHERE $7 >= q.window_start
             AND ($7 >= q.window_end OR q.used <= $8 - excluded.used)
         RETURNING window_start, window_end, used"
    )
});

impl QuotaRepository {
    pub fn new() -> Self {
        Self
    }

    /// Charge the active window using the latest persisted gateway version.
    /// Keep the stored bounds until they expire. The first window of a new
    /// calendar starts at the old boundary if the calendars overlap.
    pub async fn reserve(
        &self,
        pool: &Pool,
        reservation: QuotaReservation<'_>,
    ) -> Result<QuotaReserved> {
        let QuotaReservation {
            owner,
            bucket,
            config_version,
            now,
            window_start,
            window_end,
            limit,
            units,
        } = reservation;
        // The insert has no condition: a fresh balance must fit on its own.
        ensure!(units <= limit, "quota reservation exceeds its limit");
        ensure!(
            window_start <= now && now < window_end,
            "invalid quota bounds"
        );
        let stored_limit = i64::try_from(limit).context("quota limit exceeds i64")?;
        let units = i64::try_from(units).context("quota reservation exceeds i64")?;
        let row = sqlx::query_as(sqlx::AssertSqlSafe(RESERVE_SQL.as_str()))
            .bind(bucket)
            .bind(owner.kind())
            .bind(owner.id())
            .bind(window_start)
            .bind(window_end)
            .bind(units)
            .bind(now)
            .bind(stored_limit)
            .bind(config_version)
            .fetch_optional(pool)
            .await?;
        if let Some(row) = row {
            return Ok(QuotaReserved::Granted(QuotaBalance::from_row(row)?));
        }

        // Read once on refusal so the tracker can cache a full balance.
        let (version, start, end, used): (i64, Option<i64>, Option<i64>, Option<i64>) =
            sqlx::query_as(
                "SELECT c.version, q.window_start, q.window_end, q.used FROM quota_config c
             LEFT JOIN quota_usage q ON q.bucket = $1 WHERE c.id = 1",
            )
            .bind(bucket)
            .fetch_one(pool)
            .await?;
        if version != config_version {
            return Ok(QuotaReserved::StaleConfig);
        }
        let (Some(start), Some(end), Some(used)) = (start, end, used) else {
            anyhow::bail!("quota balance disappeared after a refused reservation");
        };
        Ok(QuotaReserved::Refused(QuotaBalance::from_row((
            start, end, used,
        ))?))
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
    use super::{
        QuotaBalance, QuotaOwner, QuotaRepository, QuotaReservation, QuotaReserved, Result,
    };
    use crate::backend::Pool;
    use crate::ent::{
        ConfigDocument, ConfigKind, ConfigSaveOutcome, ConfigSource, TrustedAuditContext,
    };
    use crate::{DbStore, service::Service};

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
                    config_version: 0,
                    now: window_start,
                    window_start,
                    window_end,
                    limit,
                    units,
                },
            )
            .await
    }

    pub(super) fn balance(window_start: i64, window_end: i64, used: u64) -> QuotaBalance {
        QuotaBalance {
            window_start,
            window_end,
            used,
        }
    }

    async fn save_gateway(pool: &Pool, content: &str, expected: Option<String>) -> ConfigDocument {
        let outcome = Service::new(pool.clone())
            .save_config_document(
                ConfigKind::Gateway,
                content.to_owned(),
                expected,
                ConfigSource::CliImport,
                TrustedAuditContext::system_control_plane(),
            )
            .await
            .unwrap();
        let ConfigSaveOutcome::Saved(document) = outcome else {
            panic!("expected a new gateway version, got {outcome:?}");
        };
        document
    }

    pub(super) async fn verify_calendar_transition(pool: &Pool) {
        use QuotaReserved::{Granted, Refused, StaleConfig};

        let bucket = bucket();
        let repo = QuotaRepository::new();
        let first = save_gateway(pool, "day", None).await;
        let old = || QuotaReservation {
            owner: OWNER,
            bucket: &bucket,
            config_version: first.version,
            now: 170,
            window_start: 100,
            window_end: 200,
            limit: 10,
            units: 5,
        };
        assert_eq!(
            repo.reserve(pool, old()).await.unwrap(),
            Granted(balance(100, 200, 5))
        );
        let delayed = old();
        let second = save_gateway(pool, "month", Some(first.revision.clone())).await;
        let new = |now| QuotaReservation {
            config_version: second.version,
            now,
            window_start: 150,
            window_end: 400,
            ..old()
        };

        // A reservation prepared in the previous generation cannot charge
        // after the save, even before B makes its first reservation.
        assert_eq!(repo.reserve(pool, delayed).await.unwrap(), StaleConfig);
        assert_eq!(
            repo.reserve(
                pool,
                QuotaReservation {
                    bucket: "untouched",
                    ..old()
                }
            )
            .await
            .unwrap(),
            StaleConfig,
        );
        assert_eq!(
            repo.reserve(pool, new(175)).await.unwrap(),
            Granted(balance(100, 200, 10))
        );
        for _ in 0..2 {
            assert_eq!(repo.reserve(pool, old()).await.unwrap(), StaleConfig);
            assert_eq!(
                repo.reserve(pool, new(199)).await.unwrap(),
                Refused(balance(100, 200, 10))
            );
        }

        assert_eq!(
            repo.reserve(pool, new(200)).await.unwrap(),
            Granted(balance(200, 400, 5))
        );
        assert_eq!(
            repo.reserve(
                pool,
                QuotaReservation {
                    now: 200,
                    window_start: 200,
                    window_end: 300,
                    ..old()
                }
            )
            .await
            .unwrap(),
            StaleConfig,
        );
        assert_eq!(
            repo.reserve(pool, new(199)).await.unwrap(),
            Refused(balance(200, 400, 5))
        );
        assert_eq!(
            repo.reserve(pool, new(201)).await.unwrap(),
            Granted(balance(200, 400, 10))
        );
        assert_eq!(
            repo.reserve(pool, new(201)).await.unwrap(),
            Refused(balance(200, 400, 10))
        );
    }

    pub(super) async fn verify_maximum_balance(pool: &Pool) {
        use QuotaReserved::{Granted, Refused};
        const LIMIT: u64 = i64::MAX as u64;
        let bucket = bucket();
        assert_eq!(
            reserve(pool, &bucket, 100, 200, LIMIT, LIMIT)
                .await
                .unwrap(),
            Granted(balance(100, 200, LIMIT))
        );
        for _ in 0..2 {
            assert_eq!(
                reserve(pool, &bucket, 100, 200, LIMIT, 1).await.unwrap(),
                Refused(balance(100, 200, LIMIT))
            );
        }
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
                        config_version: 0,
                        now: 100,
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
    use super::test_support::{self, balance, bucket, reserve};
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
            reserve(pool, &bucket, 100, 200, 10, 4).await.unwrap(),
            Granted(balance(100, 200, 4))
        );
        assert_eq!(
            reserve(pool, &bucket, 100, 200, 10, 7).await.unwrap(),
            Refused(balance(100, 200, 4))
        );
        assert_eq!(
            reserve(pool, &bucket, 100, 200, 10, 6).await.unwrap(),
            Granted(balance(100, 200, 10))
        );
        assert!(
            reserve(pool, &bucket, 100, 200, 10, 11).await.is_err(),
            "a batch larger than the limit is a caller error"
        );
    }

    #[tokio::test]
    async fn a_new_window_replaces_the_old_balance() {
        let database = TestDatabase::new().await;
        let pool = &database.pool;
        let bucket = bucket();
        assert_eq!(
            reserve(pool, &bucket, 100, 200, 10, 7).await.unwrap(),
            Granted(balance(100, 200, 7))
        );
        assert_eq!(
            reserve(pool, &bucket, 200, 300, 10, 3).await.unwrap(),
            Granted(balance(200, 300, 3))
        );
        // A stale clock cannot roll the balance back to the previous window.
        assert_eq!(
            reserve(pool, &bucket, 100, 200, 10, 1).await.unwrap(),
            Refused(balance(200, 300, 3))
        );
    }

    #[tokio::test]
    async fn calendar_changes_keep_the_active_balance_and_fence_old_versions() {
        let database = TestDatabase::new().await;
        test_support::verify_calendar_transition(&database.pool).await;
    }

    #[tokio::test]
    async fn a_maximum_balance_refuses_further_charges() {
        let database = TestDatabase::new().await;
        test_support::verify_maximum_balance(&database.pool).await;
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
                    reserve(&pool, &bucket, 100, 200, 10, 1).await.unwrap(),
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
    use super::QuotaReserved::{Granted, Refused};
    use super::test_support::{self, balance, bucket, reserve};
    use crate::backend::Pool;
    use sqlx::postgres::PgConnectOptions;

    async fn with_database(test: impl AsyncFnOnce(&Pool)) {
        dotenvy::dotenv().ok();
        let url = std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .expect("TEST_DATABASE_URL or DATABASE_URL must be set");
        let control = Pool::connect(&url).await.unwrap();
        let schema = format!(
            "quota_{}",
            ulid::Ulid::generate().to_string().to_lowercase()
        );
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&control)
            .await
            .unwrap();
        let options = url
            .parse::<PgConnectOptions>()
            .unwrap()
            .options([("search_path", &schema)]);
        let pool = Pool::connect_with(options).await.unwrap();
        crate::service::Service::new(pool.clone())
            .migrate()
            .await
            .unwrap();
        test_support::create_owner(&pool).await;
        test(&pool).await;
        pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&control)
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn postgres_reservations_are_atomic_across_connections() {
        with_database(async |pool| {
            let bucket = bucket();
            let mut tasks = tokio::task::JoinSet::new();
            for _ in 0..32 {
                let pool = pool.clone();
                let bucket = bucket.clone();
                tasks.spawn(async move {
                    matches!(
                        reserve(&pool, &bucket, 100, 200, 10, 1).await.unwrap(),
                        Granted(_)
                    )
                });
            }
            let mut granted = 0;
            while let Some(result) = tasks.join_next().await {
                granted += usize::from(result.unwrap());
            }
            assert_eq!(granted, 10);
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn postgres_refusals_report_the_blocking_balance() {
        with_database(async |pool| {
            let bucket = bucket();
            assert_eq!(
                reserve(pool, &bucket, 200, 300, 10, 7).await.unwrap(),
                Granted(balance(200, 300, 7))
            );
            assert_eq!(
                reserve(pool, &bucket, 200, 300, 10, 4).await.unwrap(),
                Refused(balance(200, 300, 7))
            );
            assert_eq!(
                reserve(pool, &bucket, 100, 200, 10, 1).await.unwrap(),
                Refused(balance(200, 300, 7))
            );
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn postgres_calendar_changes_keep_the_active_balance_and_fence_old_versions() {
        with_database(test_support::verify_calendar_transition).await;
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn postgres_maximum_balance_refuses_further_charges() {
        with_database(test_support::verify_maximum_balance).await;
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn postgres_pruning_removes_only_the_balances_of_deleted_owners() {
        with_database(async |pool| {
            assert_eq!(
                test_support::prune_after_deleting(pool).await,
                [true, true, true, false, false]
            );
        })
        .await;
    }
}
