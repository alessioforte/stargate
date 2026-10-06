use crate::api::gateway::upstream::attempt::{DispatchAttempt, dispatch_kind_label};
use crate::api::gateway::upstream::headers::sanitize_internal_request_headers;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::{
    internal_context::{InternalContextRuntime, SigningQueueError},
    observability::telemetry,
    request_context::{INTERNAL_CONTEXT_HEADER, PropagationDraft},
};
use ctx::{DispatchKind, IssueError};
use std::{sync::Arc, time::Duration};

/// Unsigned inputs for one selected network attempt. Cloning this value never
/// clones or reuses a compact token; [`prepare`] mints after the final URI and
/// headers are known.
#[derive(Clone)]
pub(in crate::api::gateway) struct InternalDispatch {
    service: String,
    audience: String,
    draft: Arc<PropagationDraft>,
    runtime: Arc<InternalContextRuntime>,
    kind: DispatchKind,
    attempt: u16,
}

impl InternalDispatch {
    pub(in crate::api::gateway) fn new(
        service: &str,
        audience: &str,
        draft: Option<&Arc<PropagationDraft>>,
        runtime: Option<&Arc<InternalContextRuntime>>,
        attempt: DispatchAttempt,
    ) -> Result<Self, ErrorResponse> {
        let draft = draft
            .cloned()
            .ok_or_else(|| internal_failure(service, attempt.kind, "draft", None, None))?;
        let runtime = runtime
            .cloned()
            .ok_or_else(|| internal_failure(service, attempt.kind, "runtime", None, None))?;
        Ok(Self {
            service: service.to_owned(),
            audience: audience.to_owned(),
            draft,
            runtime,
            kind: attempt.kind,
            attempt: attempt.number,
        })
    }

    /// Apply the shared final internal-egress boundary to an HTTP request or
    /// WebSocket handshake and mint exactly one token for this attempt.
    pub(in crate::api::gateway) async fn prepare<B>(
        &self,
        req: &mut http::Request<B>,
    ) -> Result<(), ErrorResponse> {
        let uri = sanitize_internal_uri(req.uri())
            .map_err(|_| internal_failure(&self.service, self.kind, "sanitization", None, None))?;
        *req.uri_mut() = uri;

        sanitize_internal_request_headers(req.headers_mut(), self.draft.request_id())
            .map_err(|_| internal_failure(&self.service, self.kind, "sanitization", None, None))?;

        telemetry::inject_trace_context(req.headers_mut());
        let active_trace_id = telemetry::trace_id_from_span(&tracing::Span::current());
        if self.draft.trace_id() != active_trace_id.as_deref() {
            return Err(internal_failure(
                &self.service,
                self.kind,
                "trace",
                None,
                None,
            ));
        }

        let issue_request = self
            .draft
            .issue_request(
                &self.audience,
                self.kind,
                self.attempt,
                req.method().as_str(),
                req.uri().path(),
            )
            .map_err(|_| internal_failure(&self.service, self.kind, "binding", None, None))?;
        let signed = self
            .runtime
            .issue_async(issue_request)
            .await
            .map_err(|error| {
                let reason = match error {
                    SigningQueueError::Full => "signing_queue_full",
                    SigningQueueError::Timeout => "signing_queue_timeout",
                    SigningQueueError::Unavailable => "signing_workers_unavailable",
                };
                let failure = internal_failure(&self.service, self.kind, reason, None, None);
                if error == SigningQueueError::Unavailable {
                    return failure;
                }
                telemetry::record_gateway_rejection("signing");
                ErrorResponse::new(ErrorCode::GatewayOverloaded)
                    .with_param("phase", "signing")
                    .with_param("reason", reason)
            })?;
        telemetry::record_internal_context_queue(
            &self.service,
            dispatch_kind_label(self.kind),
            signed.queue_duration,
        );
        let issued = match signed.result {
            Ok(issued) => issued,
            Err(error) => {
                return Err(internal_failure(
                    &self.service,
                    self.kind,
                    issue_error_category(&error),
                    Some(signed.signing_duration),
                    None,
                ));
            }
        };
        let signing_elapsed = signed.signing_duration;
        let token_bytes = issued.compact().len();
        let value = http::HeaderValue::from_str(issued.compact()).map_err(|_| {
            internal_failure(
                &self.service,
                self.kind,
                "header",
                Some(signing_elapsed),
                Some(token_bytes),
            )
        })?;

        req.headers_mut().remove(&INTERNAL_CONTEXT_HEADER);
        req.headers_mut().insert(&INTERNAL_CONTEXT_HEADER, value);
        telemetry::record_internal_context_issue(
            &self.service,
            dispatch_kind_label(self.kind),
            "success",
            "none",
            Some(signing_elapsed),
            Some(token_bytes),
        );
        tracing::debug!(
            stargate.service = %self.service,
            stargate.dispatch.kind = dispatch_kind_label(self.kind),
            stargate.outcome = "success",
            stargate.reason = "none",
            stargate.internal_context.signing.duration_ms = signing_elapsed.as_secs_f64() * 1_000.0,
            stargate.internal_context.queue.duration_ms = signed.queue_duration.as_secs_f64() * 1_000.0,
            stargate.internal_context.token.size = token_bytes as u64,
            "Internal-context issuance completed"
        );
        Ok(())
    }
}

impl std::fmt::Debug for InternalDispatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InternalDispatch([redacted])")
    }
}

fn issue_error_category(error: &IssueError) -> &'static str {
    match error {
        IssueError::Claims(_) => "claims",
        IssueError::TimeOverflow => "time",
        IssueError::Signing => "signing",
        IssueError::TokenTooLarge => "size",
    }
}

fn internal_failure(
    service: &str,
    kind: DispatchKind,
    reason: &'static str,
    signing_elapsed: Option<Duration>,
    token_bytes: Option<usize>,
) -> ErrorResponse {
    telemetry::record_internal_context_issue(
        service,
        dispatch_kind_label(kind),
        "failure",
        reason,
        signing_elapsed,
        token_bytes,
    );
    if let (Some(signing_elapsed), Some(token_bytes)) = (signing_elapsed, token_bytes) {
        tracing::warn!(
            stargate.service = service,
            stargate.dispatch.kind = dispatch_kind_label(kind),
            stargate.outcome = "failure",
            stargate.reason = reason,
            stargate.internal_context.signing.duration_ms = signing_elapsed.as_secs_f64() * 1_000.0,
            stargate.internal_context.token.size = token_bytes as u64,
            "Internal upstream request preparation failed"
        );
    } else if let Some(signing_elapsed) = signing_elapsed {
        tracing::warn!(
            stargate.service = service,
            stargate.dispatch.kind = dispatch_kind_label(kind),
            stargate.outcome = "failure",
            stargate.reason = reason,
            stargate.internal_context.signing.duration_ms = signing_elapsed.as_secs_f64() * 1_000.0,
            "Internal upstream request preparation failed"
        );
    } else {
        tracing::warn!(
            stargate.service = service,
            stargate.dispatch.kind = dispatch_kind_label(kind),
            stargate.outcome = "failure",
            stargate.reason = reason,
            "Internal upstream request preparation failed"
        );
    }
    ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QuerySanitizationError;

fn sanitize_internal_uri(uri: &http::Uri) -> Result<http::Uri, QuerySanitizationError> {
    let Some(path_and_query) = uri.path_and_query() else {
        return Ok(uri.clone());
    };
    let Some(query) = path_and_query.query() else {
        return Ok(uri.clone());
    };
    let query = sanitize_auth_query(query)?;
    let next = if query.is_empty() {
        path_and_query.path().to_owned()
    } else {
        format!("{}?{}", path_and_query.path(), query)
    };

    let mut parts = uri.clone().into_parts();
    parts.path_and_query = Some(
        next.parse::<http::uri::PathAndQuery>()
            .map_err(|_| QuerySanitizationError)?,
    );
    http::Uri::from_parts(parts).map_err(|_| QuerySanitizationError)
}

fn sanitize_auth_query(query: &str) -> Result<String, QuerySanitizationError> {
    let mut retained = Vec::new();
    for pair in query.split('&') {
        let key = pair.split_once('=').map_or(pair, |(key, _)| key);
        if !is_auth_query_key(key)? {
            retained.push(pair);
        }
    }
    Ok(retained.join("&"))
}

fn is_auth_query_key(key: &str) -> Result<bool, QuerySanitizationError> {
    let mut decoded = Vec::with_capacity(key.len());
    let bytes = key.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(QuerySanitizationError);
        }
        let high = hex_value(bytes[index + 1]).ok_or(QuerySanitizationError)?;
        let low = hex_value(bytes[index + 2]).ok_or(QuerySanitizationError)?;
        decoded.push((high << 4) | low);
        index += 3;
    }

    Ok(matches!(
        decoded.as_slice(),
        b"token" | b"access_token" | b"jwt"
    ))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
