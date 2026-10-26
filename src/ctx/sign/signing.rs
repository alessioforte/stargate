//! Fixed signing workers keep RSA work off Tokio and bound pending contexts.
use ctx::{IssueError, IssueRequest, IssuedContext};
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender, TrySendError},
    },
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

pub(super) const WORKERS_ENV: &str = "INTERNAL_CONTEXT_SIGNING_WORKERS";
pub(super) const QUEUE_ENV: &str = "INTERNAL_CONTEXT_SIGNING_QUEUE_CAPACITY";
pub(super) const TIMEOUT_ENV: &str = "INTERNAL_CONTEXT_SIGNING_QUEUE_TIMEOUT_MS";

#[derive(Debug, Clone, Copy)]
pub(super) struct Settings {
    pub workers: usize,
    pub queue_capacity: usize,
    pub queue_timeout: Duration,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            workers: 2,
            queue_capacity: 32,
            queue_timeout: Duration::from_millis(50),
        }
    }
}
impl Settings {
    pub(super) fn from_lookup(
        lookup: &impl Fn(&str) -> Option<String>,
    ) -> Result<Self, super::InternalContextError> {
        let bounded = |name, default, maximum| {
            let value = super::settings::unsigned_or_default(lookup, name, default)?;
            if value == 0 || value > maximum {
                return Err(super::InternalContextError::InvalidSigningBudget { name, maximum });
            }
            Ok(value)
        };
        Ok(Self {
            workers: bounded(WORKERS_ENV, 2, 64)? as usize,
            queue_capacity: bounded(QUEUE_ENV, 32, 4096)? as usize,
            queue_timeout: Duration::from_millis(bounded(TIMEOUT_ENV, 50, 60_000)?),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueueError {
    Full,
    Timeout,
    Unavailable,
}

pub(crate) struct Signed {
    pub result: Result<IssuedContext, IssueError>,
    pub signing_duration: Duration,
    pub queue_duration: Duration,
}

struct Job {
    request: IssueRequest,
    queued: Instant,
    deadline: Instant,
    started: oneshot::Sender<()>,
    result: oneshot::Sender<Signed>,
}

pub(super) struct SigningWorkers {
    sender: SyncSender<Job>,
    settings: Settings,
}
impl std::fmt::Debug for SigningWorkers {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SigningWorkers")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl SigningWorkers {
    pub(super) fn start(
        settings: Settings,
        issue: impl Fn(&IssueRequest) -> Result<IssuedContext, IssueError> + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<Job>(settings.queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let issue = Arc::new(issue);
        for index in 0..settings.workers {
            let receiver = receiver.clone();
            let issue = issue.clone();
            std::thread::Builder::new()
                .name(format!("stargate-signing-{index}"))
                .spawn(move || {
                    loop {
                        // Release the receiver lock before doing CPU work; workers sign in parallel.
                        let job = receiver
                            .lock()
                            .expect("signing receiver lock poisoned")
                            .recv();
                        let Ok(job) = job else {
                            break;
                        };
                        if job.result.is_closed() || Instant::now() >= job.deadline {
                            continue;
                        }
                        let started = Instant::now();
                        if job.started.send(()).is_err() {
                            continue;
                        }
                        let result = issue(&job.request);
                        let _ = job.result.send(Signed {
                            result,
                            signing_duration: started.elapsed(),
                            queue_duration: started.duration_since(job.queued),
                        });
                    }
                })?;
        }
        Ok(Self { sender, settings })
    }

    pub(super) async fn issue(&self, request: IssueRequest) -> Result<Signed, QueueError> {
        let queued = Instant::now();
        let deadline = queued + self.settings.queue_timeout;
        let (started, starts) = oneshot::channel();
        let (result, reply) = oneshot::channel();
        self.sender
            .try_send(Job {
                request,
                queued,
                deadline,
                started,
                result,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => QueueError::Full,
                TrySendError::Disconnected(_) => QueueError::Unavailable,
            })?;
        match tokio::time::timeout(self.settings.queue_timeout, starts).await {
            Ok(Ok(())) => {}
            Err(_) => return Err(QueueError::Timeout),
            Ok(Err(_)) if Instant::now() >= deadline => return Err(QueueError::Timeout),
            Ok(Err(_)) => return Err(QueueError::Unavailable),
        }
        reply.await.map_err(|_| QueueError::Unavailable)
    }
}

#[cfg(test)]
mod tests;
