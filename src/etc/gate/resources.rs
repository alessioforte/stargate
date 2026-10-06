use crate::{
    err::{ErrorCode, ErrorResponse},
    etc::observability::telemetry,
};
use gate::cfg::ProcessBudgets;
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub struct ProcessResources {
    pub budgets: ProcessBudgets,
    primary: Arc<Semaphore>,
    mirror: Arc<Semaphore>,
    replay_memory: Arc<Semaphore>,
    pub shutdown: CancellationToken,
    tasks: TaskTracker,
    task_activation: Mutex<()>,
}

impl ProcessResources {
    pub fn new(budgets: ProcessBudgets) -> Arc<Self> {
        Arc::new(Self {
            primary: Arc::new(Semaphore::new(budgets.primary_concurrency)),
            mirror: Arc::new(Semaphore::new(budgets.mirror_concurrency)),
            replay_memory: Arc::new(Semaphore::new(budgets.replay_memory_bytes)),
            budgets,
            shutdown: CancellationToken::new(),
            tasks: TaskTracker::new(),
            task_activation: Mutex::new(()),
        })
    }

    pub fn admit_primary(&self) -> Result<Arc<Admission>, ErrorResponse> {
        if self.shutdown.is_cancelled() {
            return Err(ErrorResponse::new(ErrorCode::GatewayCancelled));
        }
        let permit = self.primary.clone().try_acquire_owned().map_err(|_| {
            telemetry::record_gateway_rejection("primary");
            ErrorResponse::new(ErrorCode::GatewayOverloaded)
        })?;
        Ok(Arc::new(Admission {
            _permit: ResourcePermit::new(permit, "primary", 1),
            cancellation: self.shutdown.child_token(),
        }))
    }

    pub fn admit_mirror(&self) -> Option<ResourcePermit> {
        if self.shutdown.is_cancelled() {
            return None;
        }
        self.mirror
            .clone()
            .try_acquire_owned()
            .ok()
            .map(|permit| ResourcePermit::new(permit, "mirror", 1))
    }

    pub fn reserve_replay(&self, bytes: usize) -> Result<ResourcePermit, ErrorResponse> {
        let permit = self
            .replay_memory
            .clone()
            .try_acquire_many_owned(bytes as u32)
            .map_err(|_| {
                telemetry::record_gateway_rejection("replay_memory");
                ErrorResponse::new(ErrorCode::GatewayReplayMemoryExhausted)
            })?;
        Ok(ResourcePermit::new(permit, "replay_memory", bytes as i64))
    }

    pub fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) -> bool {
        // Closing a TaskTracker alone does not prevent subsequent spawns.
        let _activation = self
            .task_activation
            .lock()
            .expect("Task activation lock poisoned");
        if self.shutdown.is_cancelled() {
            return false;
        }
        self.tasks.spawn(future);
        true
    }

    pub fn begin_shutdown(&self) {
        let _activation = self
            .task_activation
            .lock()
            .expect("Task activation lock poisoned");
        self.shutdown.cancel();
        self.tasks.close();
    }

    pub async fn wait(&self) {
        self.tasks.wait().await;
    }

    #[cfg(all(test, feature = "memory"))]
    pub fn tracked_tasks(&self) -> usize {
        self.tasks.len()
    }

    #[cfg(test)]
    pub fn available(&self) -> (usize, usize, usize) {
        (
            self.primary.available_permits(),
            self.mirror.available_permits(),
            self.replay_memory.available_permits(),
        )
    }
}

pub struct Admission {
    _permit: ResourcePermit,
    pub cancellation: CancellationToken,
}

impl Drop for Admission {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

pub struct ResourcePermit {
    _permit: OwnedSemaphorePermit,
    kind: &'static str,
    amount: i64,
}

impl ResourcePermit {
    fn new(permit: OwnedSemaphorePermit, kind: &'static str, amount: i64) -> Self {
        telemetry::record_gateway_resource(kind, amount);
        Self {
            _permit: permit,
            kind,
            amount,
        }
    }
}

impl Drop for ResourcePermit {
    fn drop(&mut self) {
        telemetry::record_gateway_resource(self.kind, -self.amount);
    }
}
