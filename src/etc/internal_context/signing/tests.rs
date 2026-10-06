use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

fn request() -> IssueRequest {
    crate::etc::internal_context::test_support::anonymous_issue_request()
}

fn blocked_worker() -> (
    SigningWorkers,
    Arc<Notify>,
    SyncSender<()>,
    Arc<AtomicUsize>,
) {
    let started = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let (release, released) = mpsc::sync_channel(1);
    let released = Mutex::new(released);
    let workers = SigningWorkers::start(
        Settings {
            workers: 1,
            queue_capacity: 1,
            queue_timeout: Duration::from_millis(20),
        },
        {
            let started = started.clone();
            let calls = calls.clone();
            move |_| {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    started.notify_one();
                    released.lock().unwrap().recv().unwrap();
                }
                Err(IssueError::Signing)
            }
        },
    )
    .unwrap();
    (workers, started, release, calls)
}

async fn issue_after_drain(workers: &SigningWorkers) -> Signed {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match workers.issue(request()).await {
                Ok(signed) => return signed,
                Err(QueueError::Full) => tokio::time::sleep(Duration::from_millis(1)).await,
                Err(error) => panic!("unexpected queue error: {error:?}"),
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn queue_is_bounded_and_cancelled_requests_are_not_signed() {
    let (workers, started, release, calls) = blocked_worker();
    let first = workers.issue(request());
    tokio::pin!(first);
    assert!(futures_util::poll!(&mut first).is_pending());
    started.notified().await;
    {
        let cancelled = workers.issue(request());
        tokio::pin!(cancelled);
        assert!(futures_util::poll!(&mut cancelled).is_pending());
        assert!(matches!(
            workers.issue(request()).await,
            Err(QueueError::Full)
        ));
        // Dropping both receivers cancels this queued job before it reaches a worker.
    }
    release.send(()).unwrap();
    assert!(matches!(
        first.await.unwrap().result,
        Err(IssueError::Signing)
    ));
    assert!(matches!(
        issue_after_drain(&workers).await.result,
        Err(IssueError::Signing)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn queue_deadline_skips_expired_work_and_recovers() {
    let (workers, started, release, calls) = blocked_worker();
    let first = workers.issue(request());
    tokio::pin!(first);
    assert!(futures_util::poll!(&mut first).is_pending());
    started.notified().await;
    assert!(matches!(
        workers.issue(request()).await,
        Err(QueueError::Timeout)
    ));
    release.send(()).unwrap();
    first.await.unwrap();
    issue_after_drain(&workers).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn signing_budgets_reject_zero_invalid_and_excessive_values() {
    for (name, maximum) in [(WORKERS_ENV, 64), (QUEUE_ENV, 4096), (TIMEOUT_ENV, 60_000)] {
        for value in [
            "0".to_owned(),
            (maximum + 1).to_string(),
            "invalid".to_owned(),
        ] {
            assert!(Settings::from_lookup(&|key| (key == name).then(|| value.clone())).is_err());
        }
        assert!(Settings::from_lookup(&|key| (key == name).then(|| maximum.to_string())).is_ok());
    }
}
