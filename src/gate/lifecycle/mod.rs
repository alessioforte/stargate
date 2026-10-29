pub(in crate::gate) mod body;

use crate::{
    err::{ErrorCode, ErrorResponse},
    obs::telemetry,
};
use gate::graph::TimeoutOverride;
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub(super) struct Execution {
    pub deadline: Option<Instant>,
    pub stream: bool,
    pub grpc: bool,
    pub preserve_stream_deadline: bool,
    pub upload_idle_override: TimeoutOverride,
    pub cancellation: CancellationToken,
    failure: Arc<Mutex<Option<&'static str>>>,
}

impl Execution {
    pub fn new(timeout: Duration, cancellation: CancellationToken) -> Self {
        Self {
            deadline: Some(Instant::now() + timeout),
            stream: false,
            grpc: false,
            preserve_stream_deadline: false,
            upload_idle_override: TimeoutOverride::Inherit,
            cancellation,
            failure: Arc::new(Mutex::new(None)),
        }
    }

    pub fn failure(&self) -> Option<ErrorResponse> {
        self.failure
            .lock()
            .expect("Execution failure lock poisoned")
            .map(|phase| self.error(phase))
    }

    pub fn error(&self, phase: &'static str) -> ErrorResponse {
        ErrorResponse::new(match phase {
            "cancelled" => ErrorCode::GatewayCancelled,
            "upload" => ErrorCode::GatewayUploadTimeout,
            "upload_body" => ErrorCode::GatewayRequestBodyFailed,
            _ => ErrorCode::GatewayTimeout,
        })
        .with_param("phase", phase)
    }

    pub fn timeout(&self, phase: &'static str) -> ErrorResponse {
        telemetry::record_gateway_timeout(phase);
        *self
            .failure
            .lock()
            .expect("Execution failure lock poisoned") = Some(phase);
        self.error(phase)
    }

    pub fn check(&self) -> Result<(), ErrorResponse> {
        if self.cancellation.is_cancelled() {
            return Err(self.error("cancelled"));
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(self.timeout("total"));
        }
        Ok(())
    }

    pub async fn run<T>(
        &self,
        phase: &'static str,
        timeout: Option<Duration>,
        future: impl Future<Output = T>,
    ) -> Result<T, ErrorResponse> {
        self.check()?;
        let phase_deadline = timeout.map(|timeout| Instant::now() + timeout);
        let deadline = match (self.deadline, phase_deadline) {
            (Some(total), Some(phase)) => Some(total.min(phase)),
            (total, phase) => total.or(phase),
        };
        let phase = if deadline == self.deadline {
            "total"
        } else {
            phase
        };
        self.run_until(phase, wait_for_deadline(deadline), future)
            .await
    }

    pub async fn run_with_deadline_updates<T>(
        &self,
        mut deadlines: tokio::sync::watch::Receiver<Option<Instant>>,
        future: impl Future<Output = T>,
    ) -> Result<T, ErrorResponse> {
        let deadline = async move {
            let mut updates_open = true;
            loop {
                let deadline = *deadlines.borrow_and_update();
                tokio::select! {
                    biased;
                    changed = deadlines.changed(), if updates_open => {
                        updates_open = changed.is_ok();
                    }
                    _ = wait_for_deadline(deadline) => return,
                }
            }
        };
        self.run_until("total", deadline, future).await
    }

    async fn run_until<T>(
        &self,
        phase: &'static str,
        deadline: impl Future<Output = ()>,
        future: impl Future<Output = T>,
    ) -> Result<T, ErrorResponse> {
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => Err(self.error("cancelled")),
            _ = deadline => Err(self.timeout(phase)),
            result = future => Ok(result),
        }
    }

    pub fn upload_idle(&self, default: Duration) -> Option<Duration> {
        match self.upload_idle_override {
            TimeoutOverride::Inherit => Some(default),
            TimeoutOverride::Disabled => None,
            TimeoutOverride::Duration(duration) => Some(duration),
        }
    }

    pub fn for_mirror(&self, timeout: Duration, cancellation: CancellationToken) -> Self {
        let deadline = Instant::now() + timeout;
        Self {
            deadline: Some(if self.grpc {
                self.deadline
                    .map_or(deadline, |primary| primary.min(deadline))
            } else {
                deadline
            }),
            stream: false,
            preserve_stream_deadline: false,
            upload_idle_override: TimeoutOverride::Inherit,
            cancellation,
            failure: Arc::new(Mutex::new(None)),
            ..self.clone()
        }
    }

    pub fn response(&self) -> Self {
        let mut execution = self.clone();
        if execution.stream && !execution.preserve_stream_deadline {
            execution.deadline = None;
        }
        execution
    }
}

async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
impl Default for Execution {
    fn default() -> Self {
        Self::new(Duration::from_secs(60), CancellationToken::new())
    }
}

#[cfg(test)]
mod tests;
