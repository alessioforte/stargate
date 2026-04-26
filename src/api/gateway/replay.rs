use super::types::ReplayRequest;
use crate::err::{ErrorResponse, HttpError};
use ::http::{HeaderMap, Request, Uri, header::CONTENT_LENGTH};
use axum::body::Body;
use http_body_util::BodyExt;
use hyper::body::Bytes;

const DEFAULT_REPLAY_BODY_LIMIT: usize = 2 * 1024 * 1024;

pub(super) fn replay_body_limit() -> usize {
    std::env::var("GATEWAY_REPLAY_BODY_LIMIT")
        .ok()
        .and_then(|value| parse_byte_size(&value))
        .unwrap_or(DEFAULT_REPLAY_BODY_LIMIT)
}

pub(super) fn content_length_exceeds(headers: &HeaderMap, limit: usize) -> bool {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|value| value > limit)
}

fn parse_byte_size(value: &str) -> Option<usize> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let split_at = value
        .find(|ch: char| !ch.is_ascii_digit() && ch != '_')
        .unwrap_or(value.len());
    let number = value[..split_at].replace('_', "");
    let unit = value[split_at..].trim().to_ascii_lowercase();
    let number = number.parse::<usize>().ok()?;
    let multiplier = match unit.as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        _ => return None,
    };

    number.checked_mul(multiplier)
}

impl ReplayRequest {
    pub(super) fn build(&self, uri: &str) -> Result<Request<Body>, ErrorResponse> {
        let uri = uri.parse::<Uri>().map_err(|error| {
            tracing::error!(%uri, %error, "Invalid upstream URI");
            ErrorResponse::from(HttpError::BadGateway(
                "Failed to connect to backend service".to_string(),
            ))
        })?;

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
    limit: usize,
) -> Result<ReplayRequest, ErrorResponse> {
    let (parts, mut body) = req.into_parts();
    let mut bytes = Vec::new();

    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|error| {
            tracing::error!(%error, "Failed to buffer request body for replay");
            ErrorResponse::from(HttpError::BadRequest(
                "Failed to buffer request body".to_string(),
            ))
        })?;

        let Ok(data) = frame.into_data() else {
            continue;
        };

        let next_len = bytes.len().saturating_add(data.len());
        if next_len > limit {
            return Err(ErrorResponse::from(HttpError::PayloadTooLarge(format!(
                "Replay body limit exceeded: {} bytes",
                limit
            ))));
        }
        bytes.extend_from_slice(&data);
    }

    Ok(ReplayRequest {
        method: parts.method,
        version: parts.version,
        headers: parts.headers,
        body: Bytes::from(bytes),
    })
}
