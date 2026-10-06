use super::*;
use crate::etc::gate::resources::ProcessResources;
use futures_util::stream;
use gate::cfg::RuntimeSettings;
use http_body_util::BodyExt;
use std::convert::Infallible;

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
    let resources = ProcessResources::new(RuntimeSettings::default().compile().unwrap().budgets);
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
    let resources = ProcessResources::new(RuntimeSettings::default().compile().unwrap().budgets);
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
