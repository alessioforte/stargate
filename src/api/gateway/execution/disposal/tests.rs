use super::*;
use futures_util::stream;
use hyper::body::{Bytes, Frame};
use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};

enum Tail {
    End,
    Pending,
    Repeat(Bytes),
}

struct TrackedBody {
    frames: VecDeque<Result<Frame<Bytes>, io::Error>>,
    tail: Tail,
    polls: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
}

impl hyper::body::Body for TrackedBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        if let Some(frame) = self.frames.pop_front() {
            return Poll::Ready(Some(frame));
        }
        match &self.tail {
            Tail::End => Poll::Ready(None),
            Tail::Pending => Poll::Pending,
            Tail::Repeat(bytes) => Poll::Ready(Some(Ok(Frame::data(bytes.clone())))),
        }
    }
}

impl Drop for TrackedBody {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

fn tracked(
    frames: Vec<Result<Frame<Bytes>, io::Error>>,
    tail: Tail,
) -> (Body, Arc<AtomicUsize>, Arc<AtomicBool>) {
    let polls = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let body = Body::new(TrackedBody {
        frames: frames.into(),
        tail,
        polls: polls.clone(),
        dropped: dropped.clone(),
    });
    (body, polls, dropped)
}

#[tokio::test]
async fn small_body_drains_data_and_trailers_without_retaining_frames() {
    let (body, polls, dropped) = tracked(
        vec![
            Ok(Frame::data(Bytes::from_static(b"hello"))),
            Ok(Frame::data(Bytes::from_static(b"world"))),
            Ok(Frame::trailers(http::HeaderMap::new())),
        ],
        Tail::End,
    );
    let result = discard_body(body, 64, DISCARDED_BODY_TIMEOUT, Execution::default()).await;
    assert_eq!(
        result,
        DisposalResult {
            outcome: DisposalOutcome::Drained,
            bytes: 10
        }
    );
    assert_eq!(polls.load(Ordering::SeqCst), 4);
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(
        discard_body(
            Body::empty(),
            64,
            DISCARDED_BODY_TIMEOUT,
            Execution::default()
        )
        .await
        .outcome,
        DisposalOutcome::Drained
    );
}

#[tokio::test]
async fn endless_data_stops_at_the_first_frame_reaching_the_byte_budget() {
    for (limit, expected_bytes, expected_polls) in [(8, 8, 2), (9, 12, 3), (1, 4, 1)] {
        let (body, polls, dropped) = tracked(Vec::new(), Tail::Repeat(Bytes::from_static(b"data")));
        let result = discard_body(body, limit, DISCARDED_BODY_TIMEOUT, Execution::default()).await;
        assert_eq!(
            result,
            DisposalResult {
                outcome: DisposalOutcome::ByteLimit,
                bytes: expected_bytes
            }
        );
        assert_eq!(polls.load(Ordering::SeqCst), expected_polls);
        assert!(dropped.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn disposal_counts_frames_even_when_content_length_claims_an_empty_body() {
    let (body, polls, dropped) = tracked(Vec::new(), Tail::Repeat(Bytes::from_static(&[0; 1024])));
    let response = Response::builder()
        .header("content-length", "0")
        .body(body)
        .unwrap();
    assert_eq!(
        discard_response(
            response,
            "failover",
            &gate::cfg::RuntimeSettings::default().compile().unwrap(),
            &crate::api::gateway::lifecycle::Execution::default()
        )
        .await,
        DisposalOutcome::ByteLimit
    );
    assert_eq!(polls.load(Ordering::SeqCst), 64);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test(start_paused = true)]
async fn stalled_body_times_out_and_is_dropped() {
    let (body, _, dropped) = tracked(Vec::new(), Tail::Pending);
    let started = Instant::now();
    let result = discard_body(body, 64, DISCARDED_BODY_TIMEOUT, Execution::default()).await;
    assert_eq!(
        result,
        DisposalResult {
            outcome: DisposalOutcome::Timeout,
            bytes: 0
        }
    );
    assert_eq!(started.elapsed(), DISCARDED_BODY_TIMEOUT);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test(start_paused = true)]
async fn progress_does_not_reset_the_absolute_deadline() {
    let body = Body::from_stream(stream::unfold((), |()| async {
        tokio::time::sleep(Duration::from_millis(40)).await;
        Some((Ok::<_, io::Error>(Bytes::from_static(b"x")), ()))
    }));
    let started = Instant::now();
    let result = discard_body(body, 64, DISCARDED_BODY_TIMEOUT, Execution::default()).await;
    assert_eq!(result.outcome, DisposalOutcome::Timeout);
    assert_eq!(result.bytes, 6);
    assert_eq!(started.elapsed(), DISCARDED_BODY_TIMEOUT);
}

#[tokio::test(start_paused = true)]
async fn always_ready_empty_frames_cannot_starve_the_deadline() {
    let (body, polls, dropped) = tracked(Vec::new(), Tail::Repeat(Bytes::new()));
    let task = tokio::spawn(discard_body(
        body,
        64,
        DISCARDED_BODY_TIMEOUT,
        Execution::default(),
    ));
    tokio::task::yield_now().await;
    assert!(polls.load(Ordering::SeqCst) > 0);
    tokio::time::advance(DISCARDED_BODY_TIMEOUT).await;
    assert_eq!(task.await.unwrap().outcome, DisposalOutcome::Timeout);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn body_error_drops_the_body_without_polling_the_remaining_frames() {
    let (body, polls, dropped) = tracked(
        vec![
            Ok(Frame::data(Bytes::from_static(b"data"))),
            Err(io::Error::other("upstream body failed")),
            Ok(Frame::data(Bytes::from_static(b"unread"))),
        ],
        Tail::End,
    );
    let result = discard_body(body, 64, DISCARDED_BODY_TIMEOUT, Execution::default()).await;
    assert_eq!(
        result,
        DisposalResult {
            outcome: DisposalOutcome::BodyError,
            bytes: 4
        }
    );
    assert_eq!(polls.load(Ordering::SeqCst), 2);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn cancellation_drops_a_stalled_body() {
    let (body, polls, dropped) = tracked(Vec::new(), Tail::Pending);
    let task = tokio::spawn(discard_body(
        body,
        64,
        DISCARDED_BODY_TIMEOUT,
        Execution::default(),
    ));
    tokio::task::yield_now().await;
    assert!(polls.load(Ordering::SeqCst) > 0);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(dropped.load(Ordering::SeqCst));
}
