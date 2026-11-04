use super::HashError;
use opentelemetry::{KeyValue, metrics::Counter};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const WAIT: Duration = Duration::from_secs(5);
static BUDGET: LazyLock<Arc<Budget>> = LazyLock::new(|| {
    let default = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let concurrency =
        crate::conf::env::parse_or::<NonZeroUsize>("PASSWORD_HASH_CONCURRENCY", default)
            .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        concurrency.get() <= Semaphore::MAX_PERMITS,
        "PASSWORD_HASH_CONCURRENCY exceeds the semaphore capacity"
    );
    Arc::new(Budget::new(concurrency.get(), WAIT))
});

pub(super) struct Budget {
    semaphore: Arc<Semaphore>,
    active: Arc<AtomicU64>,
    rejections: Counter<u64>,
    wait: Duration,
}
impl Budget {
    fn new(concurrency: usize, wait: Duration) -> Self {
        let meter = crate::obs::telemetry::meter();
        let active = Arc::new(AtomicU64::new(0));
        let observed = active.clone();
        meter
            .u64_observable_gauge("stargate.iam.password_hashing.active")
            .with_description("Password hashing jobs executing on the blocking pool")
            .with_callback(move |observer| observer.observe(observed.load(Ordering::Relaxed), &[]))
            .build();
        Self {
            semaphore: Arc::new(Semaphore::new(concurrency)),
            active,
            rejections: meter
                .u64_counter("stargate.iam.password_hashing.rejections")
                .with_description(
                    "Password hashing admission timeouts and skipped background rehashes",
                )
                .build(),
            wait,
        }
    }
    fn rejected(&self, operation: &'static str) {
        self.rejections
            .add(1, &[KeyValue::new("stargate.operation", operation)]);
    }
    pub(super) async fn run<T: Send + 'static>(
        &self,
        operation: &'static str,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, HashError> {
        let permit = tokio::time::timeout(self.wait, self.semaphore.clone().acquire_owned())
            .await
            .map_err(|_| {
                self.rejected(operation);
                HashError::Overloaded
            })?
            .map_err(|_| HashError::Failed)?;
        self.execute(permit, work).await
    }
    pub(super) async fn rehash<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<Option<T>, HashError> {
        let Ok(permit) = self.semaphore.clone().try_acquire_owned() else {
            self.rejected("rehash_skipped");
            return Ok(None);
        };
        self.execute(permit, work).await.map(Some)
    }
    async fn execute<T: Send + 'static>(
        &self,
        permit: OwnedSemaphorePermit,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, HashError> {
        let active = self.active.clone();
        tokio::task::spawn_blocking(move || {
            // Blocking work survives cancellation of the awaiting request. Its
            // permit and activity guard must survive with it, including panics.
            let _permit = permit;
            active.fetch_add(1, Ordering::Relaxed);
            let _activity = Activity(active);
            work()
        })
        .await
        .map_err(|error| {
            tracing::error!(%error, "password hashing task failed");
            HashError::Failed
        })
    }
}
struct Activity(Arc<AtomicU64>);
impl Drop for Activity {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}
pub(super) fn current() -> Arc<Budget> {
    #[cfg(test)]
    if let Ok(budget) = TEST_BUDGET.try_with(Clone::clone) {
        return budget;
    }
    BUDGET.clone()
}
#[cfg(test)]
tokio::task_local! { static TEST_BUDGET: Arc<Budget>; }
#[cfg(test)]
pub(crate) async fn exhausted<F: std::future::Future>(future: F) -> F::Output {
    let budget = Arc::new(Budget::new(1, Duration::ZERO));
    let _permit = budget.semaphore.clone().acquire_owned().await.unwrap();
    TEST_BUDGET.scope(budget, future).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn admission_expires_after_five_seconds_and_rehash_never_waits() {
        let budget = Budget::new(1, WAIT);
        let _permit = budget.semaphore.clone().acquire_owned().await.unwrap();
        let start = tokio::time::Instant::now();
        assert_eq!(budget.rehash(|| unreachable!()).await.unwrap(), None::<()>);
        assert_eq!(tokio::time::Instant::now(), start);
        assert!(matches!(
            budget.run("verify", || unreachable!()).await,
            Err(HashError::Overloaded)
        ));
        assert_eq!(start.elapsed(), WAIT);
    }
    #[tokio::test]
    async fn cancelled_request_keeps_its_permit_until_blocking_work_finishes() {
        let budget = Arc::new(Budget::new(1, WAIT));
        let (started, ready) = tokio::sync::oneshot::channel();
        let (finish, wait) = std::sync::mpsc::channel();
        let task_budget = budget.clone();
        let request = tokio::spawn(async move {
            task_budget
                .run("hash", move || {
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                })
                .await
        });
        ready.await.unwrap();
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        assert_eq!(budget.active.load(Ordering::Relaxed), 1);
        assert!(budget.semaphore.clone().try_acquire_owned().is_err());
        finish.send(()).unwrap();
        let _released = budget.semaphore.clone().acquire_owned().await.unwrap();
        assert_eq!(budget.active.load(Ordering::Relaxed), 0);
    }
}
