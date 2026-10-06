use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::request_context::INTERNAL_CONTEXT_HEADER;
use ::http::{HeaderMap, Method, Request, Version, header::CONTENT_LENGTH};
use axum::body::Body;
use http_body_util::BodyExt;
use hyper::body::Bytes;

pub(in crate::api::gateway) fn content_length_exceeds(headers: &HeaderMap, limit: usize) -> bool {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|value| value > limit)
}

impl ReplayRequest {
    pub(in crate::api::gateway) fn build(&self, uri: &str) -> Result<Request<Body>, ErrorResponse> {
        let uri = crate::api::gateway::upstream::http::parse_upstream_uri(uri)?;

        let mut req = Request::new(Body::from(self.body.clone()));
        *req.method_mut() = self.method.clone();
        *req.version_mut() = self.version;
        *req.uri_mut() = uri;
        *req.headers_mut() = self.headers.clone();
        Ok(req)
    }
}

pub(in crate::api::gateway) async fn buffer_request(
    req: Request<Body>,
    runtime: &crate::etc::gate::RuntimeSnapshot,
    execution: &crate::api::gateway::lifecycle::Execution,
    reservation: crate::etc::gate::resources::ResourcePermit,
) -> Result<ReplayRequest, ErrorResponse> {
    let limit = runtime.settings.replay_body_bytes;
    let (mut parts, body) = req.into_parts();
    let mut body = crate::api::gateway::lifecycle::body::guarded_body(
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
mod tests;

#[derive(Debug, Clone)]
pub(in crate::api::gateway) struct ReplayRequest {
    pub(in crate::api::gateway) method: Method,
    pub(in crate::api::gateway) version: Version,
    pub(in crate::api::gateway) headers: HeaderMap,
    pub(in crate::api::gateway) body: Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::api::gateway) enum ReplayEligibility {
    Idempotent,
    SingleDispatch,
}

impl ReplayEligibility {
    pub(in crate::api::gateway) fn for_method(method: &Method) -> Self {
        // RFC 9110 sections 9.2.1–9.2.2: safe methods, PUT, and DELETE are
        // idempotent. Headers alone cannot establish an operation contract.
        if matches!(
            *method,
            Method::GET
                | Method::HEAD
                | Method::OPTIONS
                | Method::TRACE
                | Method::PUT
                | Method::DELETE
        ) {
            Self::Idempotent
        } else {
            Self::SingleDispatch
        }
    }
}
