use super::Execution;
use crate::{
    err::ErrorCode,
    etc::{gate::resources::Admission, observability::telemetry},
};
use axum::body::Body;
use hyper::body::{Body as _, Bytes, Frame, SizeHint};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::time::{Instant, Sleep};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

/// Timers are polled before the body, including always-ready empty frames.
/// Dropping the body immediately drops its permits and cancels its upload.
pub(in crate::api::gateway) fn guarded_body(
    body: Body,
    execution: Execution,
    idle: Duration,
    phase: &'static str,
    admission: Option<Arc<Admission>>,
) -> Body {
    // The outer downstream wrapper owns admission; inner wrappers must not
    // count the same response again. Uploads have their own lifetime.
    let mut timer = (phase == "upload" || admission.is_some())
        .then(|| TransferTimer::new(phase, execution.cancellation.clone()));
    if body.is_end_stream() {
        if let Some(timer) = &mut timer {
            timer.finish("complete");
        }
        return body;
    }
    let deadline = execution
        .deadline
        .map(|deadline| Box::pin(tokio::time::sleep_until(deadline)));
    let cancelled = Box::pin(execution.cancellation.clone().cancelled_owned());
    Body::new(ProgressBody {
        timer,
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
    // Record a disconnected body before dropping admission cancels its token.
    timer: Option<TransferTimer>,
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
    fn finish(&mut self, outcome: &'static str) {
        if let Some(timer) = &mut self.timer {
            timer.finish(outcome);
        }
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
            this.finish(if error.code == ErrorCode::GatewayCancelled {
                "cancelled"
            } else {
                "timeout"
            });
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
                    this.finish("complete");
                }
            }
            Poll::Ready(Some(Err(_))) if this.phase == "upload" => {
                *this
                    .execution
                    .failure
                    .lock()
                    .expect("Execution failure lock poisoned") = Some("upload_body");
                this.finish("error");
            }
            Poll::Ready(Some(Err(_))) => this.finish(
                if this
                    .execution
                    .failure()
                    .is_some_and(|error| error.code == ErrorCode::GatewayTimeout)
                {
                    "timeout"
                } else {
                    "error"
                },
            ),
            Poll::Ready(None) => this.finish("complete"),
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

pub(in crate::api::gateway) struct TransferTimer {
    phase: &'static str,
    started: std::time::Instant,
    cancellation: CancellationToken,
    finished: bool,
}

impl TransferTimer {
    pub(in crate::api::gateway) fn new(
        phase: &'static str,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            phase,
            started: std::time::Instant::now(),
            cancellation,
            finished: false,
        }
    }

    pub(in crate::api::gateway) fn finish(&mut self, outcome: &'static str) {
        if !self.finished {
            telemetry::record_gateway_transfer(self.phase, outcome, self.started.elapsed());
            self.finished = true;
        }
    }
}

impl Drop for TransferTimer {
    fn drop(&mut self) {
        self.finish(if self.cancellation.is_cancelled() {
            "cancelled"
        } else {
            "dropped"
        });
    }
}

#[cfg(test)]
mod tests;
