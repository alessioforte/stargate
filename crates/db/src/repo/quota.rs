//! Durable quota balance.
//!
//! A reservation charges a batch of units to the window before the replica
//! spends them. The conditional upsert is the whole concurrency control: at
//! most `limit` units are ever reserved for one bucket and window, whatever
//! the number of replicas, and a crash only strands the batch it had not
//! consumed.

use crate::backend::Pool;
use anyhow::{Context, Result, ensure};

#[derive(Clone, Copy)]
pub struct QuotaRepository;

/// One atomic reservation request.
pub struct QuotaReservation<'a> {
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
                 (bucket, definition, window_start, window_end, used)
             VALUES ($1, $2, $3, $4, $5)
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
                 OR (q.window_start = excluded.window_start AND q.used + excluded.used <= $6)
             RETURNING used",
        )
        .bind(bucket)
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
}

impl Default for QuotaRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test_support {
    use super::{QuotaRepository, QuotaReservation, QuotaReserved, Result};
    use crate::backend::Pool;

    pub(super) fn bucket() -> String {
        format!("stargate:v1:{{sub:{}}}:quota:test", ulid::Ulid::generate())
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
}

#[cfg(all(test, feature = "sqlite"))]
mod sqlite_tests {
    use super::QuotaReserved::{Granted, Refused};
    use super::test_support::{bucket, reserve};
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
}

#[cfg(all(test, feature = "postgres"))]
mod postgres_tests {
    use super::QuotaReserved::Granted;
    use super::test_support::{bucket, reserve};
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
}
