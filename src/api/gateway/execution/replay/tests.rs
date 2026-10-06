use super::*;
async fn buffer(
    req: Request<Body>,
    runtime: &crate::etc::gate::RuntimeSnapshot,
    execution: &crate::api::gateway::lifecycle::Execution,
) -> Result<ReplayRequest, ErrorResponse> {
    let reservation = runtime
        .resources
        .reserve_replay(runtime.settings.replay_body_bytes)?;
    buffer_request(req, runtime, execution, reservation).await
}

#[tokio::test]
async fn replay_buffer_never_retains_or_rebuilds_a_signed_context() {
    let mut request = Request::builder()
        .method("POST")
        .uri("/orders")
        .header("stargate-context", "stale-one")
        .header("x-request-id", "01JZ000000000000000000000R")
        .body(Body::from("payload"))
        .unwrap();
    request.headers_mut().append(
        &INTERNAL_CONTEXT_HEADER,
        http::HeaderValue::from_static("stale-two"),
    );

    let replay = buffer(
        request,
        &crate::etc::gate::test_support::runtime(
            gate::cfg::RuntimeConfig::from_raw(gate::cfg::Config::default()).unwrap(),
        ),
        &crate::api::gateway::lifecycle::Execution::default(),
    )
    .await
    .unwrap();
    let rebuilt = replay.build("http://orders.test/orders").unwrap();

    assert!(
        replay
            .headers
            .get_all(&INTERNAL_CONTEXT_HEADER)
            .iter()
            .next()
            .is_none()
    );
    assert!(rebuilt.headers().get(INTERNAL_CONTEXT_HEADER).is_none());
    assert_eq!(
        rebuilt.headers()["x-request-id"],
        "01JZ000000000000000000000R"
    );
    assert_eq!(
        rebuilt.into_body().collect().await.unwrap().to_bytes(),
        "payload"
    );
}

fn small_runtime() -> std::sync::Arc<crate::etc::gate::RuntimeSnapshot> {
    let mut config = gate::cfg::Config::default();
    config.runtime.replay_body_bytes = 8;
    config.runtime.replay_memory_bytes = 16;
    config.runtime.upload_idle_timeout = "100ms".into();
    crate::etc::gate::test_support::runtime(gate::cfg::RuntimeConfig::from_raw(config).unwrap())
}

#[tokio::test]
async fn memory_reservation_follows_last_replay_byte_owner() {
    let runtime = small_runtime();
    let replay = buffer(
        Request::new(Body::from("payload")),
        &runtime,
        &crate::api::gateway::lifecycle::Execution::default(),
    )
    .await
    .unwrap();
    assert_eq!(runtime.resources.available().2, 8);
    let rebuilt = replay.build("http://orders.test/orders").unwrap();
    let cloned = replay.clone();
    drop(replay);
    drop(cloned);
    assert_eq!(runtime.resources.available().2, 8);
    let bytes = rebuilt.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(runtime.resources.available().2, 8);
    drop(bytes);
    assert_eq!(runtime.resources.available().2, 16);
}

#[tokio::test(start_paused = true)]
async fn saturation_oversize_and_upload_timeout_release_reservations() {
    let runtime = small_runtime();
    let execution = crate::api::gateway::lifecycle::Execution::default();
    let first = buffer(Request::new(Body::from("first")), &runtime, &execution)
        .await
        .unwrap();
    let second = buffer(Request::new(Body::from("second")), &runtime, &execution)
        .await
        .unwrap();
    let error = buffer(Request::new(Body::empty()), &runtime, &execution)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::GatewayReplayMemoryExhausted);
    drop(first);
    drop(second);
    assert_eq!(runtime.resources.available().2, 16);
    let error = buffer(Request::new(Body::from("oversized")), &runtime, &execution)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::GatewayReplayPayloadTooLarge);
    assert_eq!(runtime.resources.available().2, 16);
    let body = Body::from_stream(futures_util::stream::pending::<
        Result<Bytes, std::convert::Infallible>,
    >());
    let error = buffer(Request::new(body), &runtime, &execution)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::GatewayUploadTimeout);
    assert_eq!(runtime.resources.available().2, 16);
}
