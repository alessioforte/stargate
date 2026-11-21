//! Durable quota balance.
//!
//! A reservation charges a batch of units to the window before the replica
//! spends them. The conditional update is the whole concurrency control: at
//! most `limit` units are ever reserved for one bucket and window, whatever
//! the number of replicas, and a crash only strands the batch it had not
//! consumed.

use crate::backend::Pool;
use anyhow::{Context, Result};

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

impl QuotaRepository {
    pub fn new() -> Self {
        Self
    }

    /// Reserve `units` in `bucket` for `[window_start, window_end)`.
    ///
    /// Returns the balance in use after the reservation, or `None` when the
    /// remaining balance cannot fund it. A row of an earlier window is
    /// replaced instead of accumulated, so the first request of a new window
    /// starts from zero. A reservation for a window older than the stored one
    /// with the same definition is refused: a replica with a stale clock must
    /// not roll the balance back. `definition` identifies the configured
    /// period, time zone and anchor; changing it also starts a fresh balance.
    pub async fn reserve(
        &self,
        pool: &Pool,
        reservation: QuotaReservation<'_>,
    ) -> Result<Option<u64>> {
        let QuotaReservation {
            bucket,
            definition,
            window_start,
            window_end,
            limit,
            units,
        } = reservation;
        let limit = i64::try_from(limit).context("quota limit exceeds i64")?;
        let units = i64::try_from(units).context("quota reservation exceeds i64")?;
        sqlx::query(
            "INSERT INTO quota_usage (bucket, definition, window_start, window_end, used)
             VALUES ($1, $2, $3, $4, 0)
             ON CONFLICT (bucket) DO NOTHING",
        )
        .bind(bucket)
        .bind(definition)
        .bind(window_start)
        .bind(window_end)
        .execute(pool)
        .await?;

        let used: Option<i64> = sqlx::query_scalar(
            "UPDATE quota_usage
             SET used = CASE
                     WHEN window_start = $3 AND definition = $2 THEN used + $5
                     ELSE $5
                 END,
                 definition = $2,
                 window_start = $3,
                 window_end = $4
             WHERE bucket = $1
               AND (
                     $3 > window_start
                     OR (window_start = $3 AND definition = $2)
                     OR definition <> $2
               )
               AND (
                     CASE WHEN window_start = $3 AND definition = $2 THEN used ELSE 0 END
                     + $5 <= $6
               )
             RETURNING used",
        )
        .bind(bucket)
        .bind(definition)
        .bind(window_start)
        .bind(window_end)
        .bind(units)
        .bind(limit)
        .fetch_optional(pool)
        .await?;

        Ok(used.and_then(|used| u64::try_from(used).ok()))
    }
}

impl Default for QuotaRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test_support {
    use super::{QuotaReservation, Result};

    pub(super) async fn reserve(
        service: &crate::service::Service,
        bucket: &str,
        definition: &str,
        window_start: i64,
        window_end: i64,
        limit: u64,
        units: u64,
    ) -> Result<Option<u64>> {
        service
            .reserve_quota(QuotaReservation {
                bucket,
                definition,
                window_start,
                window_end,
                limit,
                units,
            })
            .await
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod sqlite_tests {
    use super::test_support;
    use std::path::PathBuf;

    struct TestDatabase {
        url: String,
        path: PathBuf,
    }

    impl TestDatabase {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("stargate-quota-{}.db", ulid::Ulid::generate()));
            Self {
                url: format!("sqlite://{}?mode=rwc", path.display()),
                path,
            }
        }

        async fn service(&self) -> crate::service::Service {
            let service = crate::service::init(&self.url).await.unwrap();
            service.migrate().await.unwrap();
            service
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn bucket() -> String {
        format!("stargate:v1:{{sub:{}}}:quota:test", ulid::Ulid::generate())
    }

    #[tokio::test]
    async fn reservations_charge_batches_and_never_exceed_the_limit() {
        let database = TestDatabase::new();
        let service = database.service().await;
        let bucket = bucket();
        assert_eq!(
            test_support::reserve(&service, &bucket, "day:UTC", 100, 200, 10, 4)
                .await
                .unwrap(),
            Some(4)
        );
        assert_eq!(
            test_support::reserve(&service, &bucket, "day:UTC", 100, 200, 10, 7)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            test_support::reserve(&service, &bucket, "day:UTC", 100, 200, 10, 6)
                .await
                .unwrap(),
            Some(10)
        );
    }

    #[tokio::test]
    async fn a_new_window_replaces_the_old_balance() {
        let database = TestDatabase::new();
        let service = database.service().await;
        let bucket = bucket();
        assert_eq!(
            test_support::reserve(&service, &bucket, "day:UTC", 100, 200, 10, 7)
                .await
                .unwrap(),
            Some(7)
        );
        assert_eq!(
            test_support::reserve(&service, &bucket, "day:UTC", 200, 300, 10, 3)
                .await
                .unwrap(),
            Some(3)
        );
        // A stale replica cannot roll the balance back to the previous window.
        assert_eq!(
            test_support::reserve(&service, &bucket, "day:UTC", 100, 200, 10, 1)
                .await
                .unwrap(),
            None
        );
        // Changing the window definition starts a fresh balance.
        assert_eq!(
            test_support::reserve(&service, &bucket, "month:UTC:15", 200, 300, 10, 4)
                .await
                .unwrap(),
            Some(4)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_replicas_reserve_exactly_the_limit() {
        let database = TestDatabase::new();
        let service = database.service().await;
        let bucket = bucket();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..32 {
            let service = service.clone();
            let bucket = bucket.clone();
            tasks.spawn(async move {
                test_support::reserve(&service, &bucket, "day:UTC", 100, 200, 10, 1)
                    .await
                    .unwrap()
                    .is_some()
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
    use super::test_support;

    fn database_url() -> String {
        dotenvy::dotenv().ok();
        std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .expect("TEST_DATABASE_URL or DATABASE_URL must be set")
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn postgres_reservations_are_atomic_across_connections() {
        let service = crate::service::init(&database_url()).await.unwrap();
        service.migrate().await.unwrap();
        let bucket = format!("stargate:v1:{{sub:{}}}:quota:test", ulid::Ulid::generate());
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..32 {
            let service = service.clone();
            let bucket = bucket.clone();
            tasks.spawn(async move {
                test_support::reserve(&service, &bucket, "day:UTC", 100, 200, 10, 1)
                    .await
                    .unwrap()
                    .is_some()
            });
        }
        let mut granted = 0;
        while let Some(result) = tasks.join_next().await {
            granted += usize::from(result.unwrap());
        }
        assert_eq!(granted, 10);
    }
}
