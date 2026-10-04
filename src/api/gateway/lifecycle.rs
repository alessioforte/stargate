use crate::{
    err::{ErrorCode, ErrorResponse},
    etc::{gate::resources::Admission, telemetry},
};
use axum::body::Body;
use hyper::body::{Body as _, Bytes, Frame, SizeHint};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::time::{Instant, Sleep};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

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

/// Timers are polled before the body, including always-ready empty frames.
/// Dropping the body immediately drops its permits and cancels its upload.
pub(super) fn guarded_body(
    body: Body,
    execution: Execution,
    idle: Duration,
    phase: &'static str,
    admission: Option<Arc<Admission>>,
) -> Body {
    if body.is_end_stream() {
        return body;
    }
    let deadline = execution
        .deadline
        .map(|deadline| Box::pin(tokio::time::sleep_until(deadline)));
    let cancelled = Box::pin(execution.cancellation.clone().cancelled_owned());
    Body::new(ProgressBody {
        body: Some(body),
        execution,
        idle,
        phase,
        idle_timer: Box::pin(tokio::time::sleep(idle)),
        deadline,
        cancelled,
        admission,
        ready_frames: 0,
    })
}

struct ProgressBody {
    body: Option<Body>,
    execution: Execution,
    idle: Duration,
    phase: &'static str,
    idle_timer: Pin<Box<Sleep>>,
    deadline: Option<Pin<Box<Sleep>>>,
    cancelled: Pin<Box<WaitForCancellationFutureOwned>>,
    admission: Option<Arc<Admission>>,
    ready_frames: usize,
}

impl ProgressBody {
    fn finish(&mut self) {
        self.body = None;
        self.admission = None;
    }
}

impl hyper::body::Body for ProgressBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        let this = self.get_mut();
        if this.body.is_none() {
            return Poll::Ready(None);
        }
        let failure = if this.cancelled.as_mut().poll(cx).is_ready() {
            Some(this.execution.error("cancelled"))
        } else if this
            .deadline
            .as_mut()
            .is_some_and(|deadline| deadline.as_mut().poll(cx).is_ready())
        {
            Some(this.execution.timeout("total"))
        } else if this.idle_timer.as_mut().poll(cx).is_ready() {
            Some(this.execution.timeout(this.phase))
        } else {
            None
        };
        if let Some(error) = failure {
            this.finish();
            return Poll::Ready(Some(Err(axum::Error::new(error))));
        }
        if this.ready_frames == 64 {
            this.ready_frames = 0;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let result = Pin::new(this.body.as_mut().unwrap()).poll_frame(cx);
        match &result {
            Poll::Ready(Some(Ok(frame))) => {
                this.ready_frames += 1;
                if frame.data_ref().is_some_and(|data| !data.is_empty()) || frame.is_trailers() {
                    this.idle_timer.as_mut().reset(Instant::now() + this.idle);
                }
                if this.body.as_ref().unwrap().is_end_stream() {
                    this.finish();
                }
            }
            Poll::Ready(_) => this.finish(),
            Poll::Pending => this.ready_frames = 0,
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.body.as_ref().is_none_or(Body::is_end_stream)
    }
    fn size_hint(&self) -> SizeHint {
        self.body
            .as_ref()
            .map_or_else(|| SizeHint::with_exact(0), Body::size_hint)
    }
}

#[cfg(test)]
impl Default for Execution {
    fn default() -> Self {
        Self::new(Duration::from_secs(60), CancellationToken::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etc::gate::resources::ProcessResources;
    use futures_util::stream;
    use gate::cfg::RuntimeSettings;
    use http_body_util::BodyExt;
    use std::convert::Infallible;

    #[tokio::test(start_paused = true)]
    async fn failover_phases_share_one_absolute_budget() {
        let execution = Execution::new(Duration::from_secs(3), CancellationToken::new());
        execution
            .run(
                "response_headers",
                Duration::from_secs(10),
                tokio::time::sleep(Duration::from_secs(2)),
            )
            .await
            .unwrap();
        let error = execution
            .run(
                "response_headers",
                Duration::from_secs(10),
                tokio::time::sleep(Duration::from_secs(2)),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::GatewayTimeout);
        assert_eq!(error.params["phase"], "total");
    }

    #[tokio::test(start_paused = true)]
    async fn stream_body_uses_idle_policy_and_preserves_trailers() {
        let mut execution = Execution::new(Duration::from_secs(2), CancellationToken::new());
        execution.stream = true;
        let frames = stream::unfold(0, |index| async move {
            if index > 5 {
                return None;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            let frame = if index == 5 {
                let mut trailers = http::HeaderMap::new();
                trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
                Frame::trailers(trailers)
            } else {
                Frame::data(Bytes::from_static(b"data"))
            };
            Some((Ok::<_, Infallible>(frame), index + 1))
        });
        let body = Body::new(http_body_util::StreamBody::new(frames));
        let collected = guarded_body(
            body,
            execution.response(),
            Duration::from_secs(2),
            "response_body",
            None,
        )
        .collect()
        .await
        .unwrap();
        assert_eq!(collected.trailers().unwrap()["grpc-status"], "0");
        assert_eq!(collected.to_bytes().len(), 20);
    }

    #[tokio::test(start_paused = true)]
    async fn idle_timeout_and_cancellation_release_admission() {
        let resources =
            ProcessResources::new(RuntimeSettings::default().compile().unwrap().budgets);
        let admission = resources.admit_primary().unwrap();
        let execution = Execution::new(Duration::from_secs(60), admission.cancellation.clone());
        let body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
        let mut body = guarded_body(
            body,
            execution.clone(),
            Duration::from_secs(1),
            "response_body",
            Some(admission),
        );
        assert!(body.frame().await.unwrap().is_err());
        assert_eq!(
            execution.failure().unwrap().params["phase"],
            "response_body"
        );
        assert_eq!(
            resources.available().0,
            resources.budgets.primary_concurrency
        );
        // A retained, already-failed wrapper must not keep admission alive.
        assert!(body.is_end_stream());

        let admission = resources.admit_primary().unwrap();
        let execution = Execution::new(Duration::from_secs(60), admission.cancellation.clone());
        let body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
        let mut body = guarded_body(
            body,
            execution,
            Duration::from_secs(1),
            "response_body",
            Some(admission),
        );
        resources.begin_shutdown();
        assert!(body.frame().await.unwrap().is_err());
        assert_eq!(
            resources.available().0,
            resources.budgets.primary_concurrency
        );
    }

    #[tokio::test]
    async fn dropping_a_response_releases_its_permit_and_cancels_upload() {
        let resources =
            ProcessResources::new(RuntimeSettings::default().compile().unwrap().budgets);
        let admission = resources.admit_primary().unwrap();
        let cancelled = admission.cancellation.clone();
        let body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
        let body = guarded_body(
            body,
            Execution::new(Duration::from_secs(60), cancelled.clone()),
            Duration::from_secs(1),
            "response_body",
            Some(admission),
        );
        assert_eq!(
            resources.available().0,
            resources.budgets.primary_concurrency - 1
        );
        drop(body);
        assert!(cancelled.is_cancelled());
        assert_eq!(
            resources.available().0,
            resources.budgets.primary_concurrency
        );
    }

    #[tokio::test(start_paused = true)]
    async fn empty_frames_do_not_extend_upload_idle_time() {
        let execution = Execution::default();
        let frames = stream::unfold((), |_| async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Some((Ok::<_, Infallible>(Bytes::new()), ()))
        });
        let body = guarded_body(
            Body::from_stream(frames),
            execution.clone(),
            Duration::from_secs(1),
            "upload",
            None,
        );
        assert!(body.collect().await.is_err());
        assert_eq!(
            execution.failure().unwrap().code,
            ErrorCode::GatewayUploadTimeout
        );
    }

    #[tokio::test(start_paused = true)]
    async fn progress_cannot_extend_finite_body_lifetime() {
        let execution = Execution::new(Duration::from_secs(2), CancellationToken::new());
        let frames = stream::unfold((), |_| async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Some((Ok::<_, Infallible>(Bytes::from_static(b"data")), ()))
        });
        let body = guarded_body(
            Body::from_stream(frames),
            execution.clone(),
            Duration::from_secs(1),
            "response_body",
            None,
        );
        assert!(body.collect().await.is_err());
        assert_eq!(execution.failure().unwrap().params["phase"], "total");
    }
}
