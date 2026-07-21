use super::headers::sanitize_internal_request_headers;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::{
    internal_context::InternalContextRuntime,
    reqctx::{INTERNAL_CONTEXT_HEADER, PropagationDraft},
    telemetry,
};
use ctx::{DispatchKind, IssueError};
use std::sync::Arc;

/// Unsigned inputs for one selected network attempt. Cloning this value never
/// clones or reuses a compact token; [`prepare`] mints after the final URI and
/// headers are known.
#[derive(Clone)]
pub(super) struct InternalDispatch {
    audience: String,
    draft: Arc<PropagationDraft>,
    runtime: Arc<InternalContextRuntime>,
    kind: DispatchKind,
    attempt: u16,
}

impl InternalDispatch {
    pub(super) fn new(
        audience: &str,
        draft: Option<&Arc<PropagationDraft>>,
        runtime: Option<&Arc<InternalContextRuntime>>,
        kind: DispatchKind,
        attempt: u16,
    ) -> Result<Self, ErrorResponse> {
        let draft = draft.cloned().ok_or_else(|| internal_failure("draft"))?;
        let runtime = runtime
            .cloned()
            .ok_or_else(|| internal_failure("runtime"))?;
        Ok(Self {
            audience: audience.to_owned(),
            draft,
            runtime,
            kind,
            attempt,
        })
    }

    /// Apply the shared final internal-egress boundary to an HTTP request or
    /// WebSocket handshake and mint exactly one token for this attempt.
    pub(super) fn prepare<B>(&self, req: &mut http::Request<B>) -> Result<(), ErrorResponse> {
        let uri = sanitize_internal_uri(req.uri()).map_err(|_| internal_failure("sanitization"))?;
        *req.uri_mut() = uri;

        sanitize_internal_request_headers(req.headers_mut(), self.draft.request_id())
            .map_err(|_| internal_failure("sanitization"))?;

        telemetry::inject_trace_context(req.headers_mut());
        let active_trace_id = telemetry::trace_id_from_span(&tracing::Span::current());
        if self.draft.trace_id() != active_trace_id.as_deref() {
            return Err(internal_failure("trace"));
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
            .map_err(|_| internal_failure("binding"))?;
        let issued = self
            .runtime
            .issue(&issue_request)
            .map_err(|error| internal_failure(issue_error_category(&error)))?;
        let value = http::HeaderValue::from_str(issued.compact())
            .map_err(|_| internal_failure("header"))?;

        req.headers_mut().remove(&INTERNAL_CONTEXT_HEADER);
        req.headers_mut().insert(&INTERNAL_CONTEXT_HEADER, value);
        telemetry::record_internal_context_issue("success", "none");
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

fn internal_failure(reason: &'static str) -> ErrorResponse {
    telemetry::record_internal_context_issue("failure", reason);
    tracing::warn!(reason, "Internal upstream request preparation failed");
    ErrorResponse::from(HttpError::InternalServerError(
        "Internal upstream request preparation failed".to_string(),
    ))
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
mod tests {
    use super::*;
    use crate::api::gateway::types::ReplayRequest;
    use crate::etc::{guard::VerifiedIdentity, reqctx::RequestContext};
    use axum::body::Body;
    use chrono::Utc;
    use ctx::{
        ContextSigner, ContextVerifier, ExpectedRequest, SignerConfig, StaticKeyResolver,
        VerifierConfig,
    };
    use hyper::body::Bytes;
    use std::collections::HashSet;

    const ISSUER: &str = "https://stargate.test/internal-context";
    const AUDIENCE: &str = "urn:stargate:service:orders";
    const FALLBACK_AUDIENCE: &str = "urn:stargate:service:fallback";
    const SHADOW_AUDIENCE: &str = "urn:stargate:service:shadow";
    const KEY_ID: &str = "stargate-internal-test";
    const REQUEST_ID: &str = "01JZ000000000000000000000R";
    const PRIVATE_KEY: &[u8] = include_bytes!("../../../crates/ctx/tests/fixtures/private.pem");
    const PUBLIC_KEY: &[u8] = include_bytes!("../../../crates/ctx/tests/fixtures/public.pem");

    fn runtime() -> Arc<InternalContextRuntime> {
        let signer = ContextSigner::from_rsa_pem(
            SignerConfig::new(ISSUER, KEY_ID, 30).unwrap(),
            PRIVATE_KEY,
        )
        .unwrap();
        InternalContextRuntime::from_signer_for_test(signer)
    }

    fn draft() -> Arc<PropagationDraft> {
        let request = RequestContext::new(REQUEST_ID.to_owned(), Utc::now(), None, None, None);
        Arc::new(
            PropagationDraft::build(
                &VerifiedIdentity::anonymous(),
                &request,
                "POST",
                "/orders",
                "/orders",
                "orders-write",
                "orders",
                None,
            )
            .unwrap(),
        )
    }

    #[test]
    fn auth_query_sanitizer_decodes_only_keys_and_preserves_every_other_pair() {
        let query =
            "a=1&%74oken=secret&encoded=%2Fkeep%2F&access%5Ftoken=x&JWT=keep&j%77t=y&empty=&flag";

        let sanitized = sanitize_auth_query(query).unwrap();

        assert_eq!(sanitized, "a=1&encoded=%2Fkeep%2F&JWT=keep&empty=&flag");
    }

    #[test]
    fn malformed_percent_encoding_in_a_query_key_fails_closed() {
        assert!(sanitize_auth_query("safe=1&tok%en=secret").is_err());
        assert!(sanitize_auth_query("safe=1&token%=secret").is_err());
    }

    #[test]
    fn internal_uri_sanitizer_preserves_authority_path_and_safe_encoding() {
        let uri: http::Uri = "http://orders.test/v1/a%2Fb?z=%2F&jwt=secret&a=1"
            .parse()
            .unwrap();

        let sanitized = sanitize_internal_uri(&uri).unwrap();

        assert_eq!(sanitized, "http://orders.test/v1/a%2Fb?z=%2F&a=1");
    }

    #[tokio::test]
    async fn concurrent_replay_clones_mint_unique_primary_and_shadow_contexts() {
        let runtime = runtime();
        let draft = draft();
        let replay = ReplayRequest {
            method: http::Method::POST,
            version: http::Version::HTTP_11,
            headers: http::HeaderMap::new(),
            body: Bytes::from_static(b"payload"),
        };
        let expected_attempts = [
            (DispatchKind::Primary, 1_u16, AUDIENCE),
            (DispatchKind::Primary, 2_u16, FALLBACK_AUDIENCE),
            (DispatchKind::Shadow, 1_u16, SHADOW_AUDIENCE),
            (DispatchKind::Shadow, 2_u16, FALLBACK_AUDIENCE),
        ];

        let mut tasks = Vec::new();
        for (kind, attempt, audience) in expected_attempts {
            let runtime = Arc::clone(&runtime);
            let draft = Arc::clone(&draft);
            let replay = replay.clone();
            tasks.push(tokio::spawn(async move {
                let dispatch =
                    InternalDispatch::new(audience, Some(&draft), Some(&runtime), kind, attempt)
                        .unwrap();
                let mut request: http::Request<Body> = replay
                    .build("http://orders.test/orders?jwt=secret&safe=1")
                    .unwrap();
                dispatch.prepare(&mut request).unwrap();
                assert_eq!(request.uri().query(), Some("safe=1"));
                assert_eq!(
                    request
                        .headers()
                        .get_all(&INTERNAL_CONTEXT_HEADER)
                        .iter()
                        .count(),
                    1
                );
                request.headers()[INTERNAL_CONTEXT_HEADER]
                    .to_str()
                    .unwrap()
                    .to_owned()
            }));
        }

        let mut dispatch_ids = HashSet::new();
        for (task, (kind, attempt, audience)) in tasks.into_iter().zip(expected_attempts) {
            let token = task.await.unwrap();
            let resolver = StaticKeyResolver::from_rsa_pem(KEY_ID, PUBLIC_KEY).unwrap();
            let verifier = ContextVerifier::new(
                VerifierConfig::new(ISSUER, audience, 5).unwrap(),
                Arc::new(resolver),
            );
            let trusted = verifier
                .verify(
                    &token,
                    ExpectedRequest {
                        method: "POST",
                        encoded_path: "/orders",
                        request_id: REQUEST_ID,
                    },
                )
                .unwrap();
            assert_eq!(trusted.context().dispatch.kind, kind);
            assert_eq!(trusted.context().dispatch.attempt, attempt);
            assert_eq!(trusted.context().request.id, REQUEST_ID);
            assert!(dispatch_ids.insert(trusted.dispatch_id().to_owned()));
        }
        assert_eq!(dispatch_ids.len(), expected_attempts.len());
    }
}
