use super::types::ReplayRequest;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::reqctx::INTERNAL_CONTEXT_HEADER;
use ::http::{HeaderMap, Request, header::CONTENT_LENGTH};
use axum::body::Body;
use http_body_util::BodyExt;
use hyper::body::Bytes;

pub(super) fn content_length_exceeds(headers: &HeaderMap, limit: usize) -> bool {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|value| value > limit)
}

impl ReplayRequest {
    pub(super) fn build(&self, uri: &str) -> Result<Request<Body>, ErrorResponse> {
        let uri = super::http::parse_upstream_uri(uri)?;

        let mut req = Request::new(Body::from(self.body.clone()));
        *req.method_mut() = self.method.clone();
        *req.version_mut() = self.version;
        *req.uri_mut() = uri;
        *req.headers_mut() = self.headers.clone();
        Ok(req)
    }
}

pub(super) async fn buffer_request(
    req: Request<Body>,
    runtime: &crate::etc::gate::RuntimeSnapshot,
    execution: &super::lifecycle::Execution,
    reservation: crate::etc::gate::resources::ResourcePermit,
) -> Result<ReplayRequest, ErrorResponse> {
    let limit = runtime.settings.replay_body_bytes;
    let (mut parts, body) = req.into_parts();
    let mut body = super::lifecycle::guarded_body(
        body,
        execution.clone(),
        runtime.settings.upload_idle_timeout,
        "upload",
        None,
    );
    // Defense in depth: replay state is always unsigned. A token is minted
    // only after the concrete leaf, dispatch kind, and attempt are known.
    parts.headers.remove(&INTERNAL_CONTEXT_HEADER);
    let mut bytes = Vec::with_capacity(limit);

    loop {
        tokio::task::consume_budget().await;
        let Some(frame) = execution
            .run("upload", runtime.settings.upload_idle_timeout, body.frame())
            .await?
        else {
            break;
        };
        let frame = frame.map_err(|error| {
            tracing::error!(%error, "Failed to buffer request body for replay");
            execution
                .failure()
                .unwrap_or_else(|| ErrorResponse::new(ErrorCode::GatewayReplayBufferFailed))
        })?;

        let Ok(data) = frame.into_data() else {
            continue;
        };

        let next_len = bytes.len().saturating_add(data.len());
        if next_len > limit {
            return Err(ErrorResponse::new(ErrorCode::GatewayReplayPayloadTooLarge)
                .with_param("limitBytes", limit));
        }
        bytes.extend_from_slice(&data);
    }

    Ok(ReplayRequest {
        method: parts.method,
        version: parts.version,
        headers: parts.headers,
        body: Bytes::from_owner(ReplayStorage {
            bytes,
            _reservation: reservation,
        }),
    })
}

struct ReplayStorage {
    bytes: Vec<u8>,
    _reservation: crate::etc::gate::resources::ResourcePermit,
}
impl AsRef<[u8]> for ReplayStorage {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn buffer(
        req: Request<Body>,
        runtime: &crate::etc::gate::RuntimeSnapshot,
        execution: &super::super::lifecycle::Execution,
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
            &super::super::lifecycle::Execution::default(),
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

    #[test]
    fn transport_only_failover_requires_replay() {
        let plan = super::super::types::ExecutionPlan {
            attempts: vec![
                super::super::types::SelectedService::DirectResponse {
                    status: 502,
                    headers: Vec::new(),
                    body: None,
                },
                super::super::types::SelectedService::DirectResponse {
                    status: 200,
                    headers: Vec::new(),
                    body: None,
                },
            ],
            ..Default::default()
        };

        assert!(plan.needs_failover_replay(super::super::types::ReplayEligibility::Idempotent));
        assert!(
            !plan.needs_failover_replay(super::super::types::ReplayEligibility::SingleDispatch)
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
            &super::super::lifecycle::Execution::default(),
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
        let execution = super::super::lifecycle::Execution::default();
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
}
