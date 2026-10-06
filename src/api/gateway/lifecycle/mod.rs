pub(in crate::api::gateway) mod body;

use crate::{
    err::{ErrorCode, ErrorResponse},
    etc::observability::telemetry,
};
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
    pub cancellation: CancellationToken,
    failure: Arc<Mutex<Option<&'static str>>>,
}

impl Execution {
    pub fn new(timeout: Duration, cancellation: CancellationToken) -> Self {
        Self {
            deadline: Some(Instant::now() + timeout),
            stream: false,
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
        timeout: Duration,
        future: impl Future<Output = T>,
    ) -> Result<T, ErrorResponse> {
        self.check()?;
        let phase_deadline = Instant::now() + timeout;
        let deadline = self
            .deadline
            .map_or(phase_deadline, |total| total.min(phase_deadline));
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => Err(self.error("cancelled")),
            _ = tokio::time::sleep_until(deadline) => Err(self.timeout(if self.deadline == Some(deadline) { "total" } else { phase })),
            result = future => Ok(result),
        }
    }

    pub fn response(&self) -> Self {
        let mut execution = self.clone();
        if execution.stream {
            execution.deadline = None;
        }
        execution
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
