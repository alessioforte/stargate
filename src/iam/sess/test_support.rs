use std::sync::atomic::{AtomicBool, Ordering};
use std::{future::Future, sync::Arc};
use tokio::sync::Notify;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    Registration,
    IndexRead,
    ProfileRead,
    SubjectWrite,
}

tokio::task_local! {
    static PAUSE: Arc<Pause>;
}

pub(crate) struct Pause {
    stage: Stage,
    armed: AtomicBool,
    reached: Notify,
    resume: Notify,
}

impl Pause {
    pub(crate) fn new(stage: Stage) -> Arc<Self> {
        Arc::new(Self {
            stage,
            armed: AtomicBool::new(true),
            reached: Notify::new(),
            resume: Notify::new(),
        })
    }

    pub(crate) fn current() -> Option<Arc<Self>> {
        PAUSE.try_with(Arc::clone).ok()
    }

    pub(crate) async fn scope<T>(self: &Arc<Self>, work: impl Future<Output = T>) -> T {
        PAUSE.scope(Arc::clone(self), work).await
    }

    pub(crate) async fn hit(&self, stage: Stage) {
        if stage == self.stage && self.armed.swap(false, Ordering::Relaxed) {
            self.reached.notify_one();
            self.resume.notified().await;
        }
    }

    pub(crate) async fn wait(&self) {
        self.reached.notified().await;
    }

    pub(crate) fn resume(&self) {
        self.resume.notify_one();
    }
}

pub(crate) async fn pause_at(stage: Stage) {
    if let Some(pause) = Pause::current() {
        pause.hit(stage).await;
    }
}

pub(crate) fn inherit_pause<T>(work: impl Future<Output = T>) -> impl Future<Output = T> {
    let pause = Pause::current();
    async move {
        match pause {
            Some(pause) => pause.scope(work).await,
            None => work.await,
        }
    }
}
