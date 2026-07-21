use super::headers::{sanitize_internal_request_headers, strip_internal_context_response};
use crate::err::{ErrorResponse, HttpError};
use crate::etc::{
    internal_context::{self, InternalContextRuntime},
    reqctx::{INTERNAL_CONTEXT_HEADER, PropagationDraft},
    telemetry,
};
use axum::{body::Body, response::Response};
use ctx::{DispatchKind, IssueError};
use http::header::{
    CONNECTION, HOST, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE, TRAILER, TRANSFER_ENCODING,
    UPGRADE,
};
use std::sync::Arc;

pub(super) struct InternalDispatch {
    audience: String,
    draft: Arc<PropagationDraft>,
    runtime: Arc<InternalContextRuntime>,
    kind: DispatchKind,
    attempt: u16,
}

impl InternalDispatch {
    pub(super) fn primary(
        audience: &str,
        draft: Option<&Arc<PropagationDraft>>,
    ) -> Result<Self, ErrorResponse> {
        let draft = draft.cloned().ok_or_else(|| internal_failure("draft"))?;
        let runtime = internal_context::runtime()
            .cloned()
            .ok_or_else(|| internal_failure("runtime"))?;
        Ok(Self {
            audience: audience.to_owned(),
            draft,
            runtime,
            kind: DispatchKind::Primary,
            attempt: 1,
        })
    }

    #[cfg(test)]
    fn for_test(
        audience: &str,
        draft: Arc<PropagationDraft>,
        runtime: Arc<InternalContextRuntime>,
    ) -> Self {
        Self {
            audience: audience.to_owned(),
            draft,
            runtime,
            kind: DispatchKind::Primary,
            attempt: 1,
        }
    }
}

pub async fn handler(
    mut req: http::Request<Body>,
    headers: &http::HeaderMap,
    uri: &str,
    client: &crate::etc::gate::HyperClient,
    preserve_host: bool,
    internal_dispatch: Option<&InternalDispatch>,
) -> Result<Response, ErrorResponse> {
    let mut upstream_uri = uri.parse::<http::Uri>().map_err(|error| {
        tracing::error!(%uri, %error, "Invalid upstream URI");
        ErrorResponse::from(HttpError::BadGateway(
            "Failed to connect to backend service".to_string(),
        ))
    })?;
    if internal_dispatch.is_some() {
        upstream_uri =
            sanitize_internal_uri(&upstream_uri).map_err(|_| internal_failure("sanitization"))?;
    }

    *req.uri_mut() = upstream_uri;
    strip_hop_by_hop_headers(req.headers_mut());
    if !preserve_host {
        req.headers_mut().remove(HOST);
    }
    if let Some(dispatch) = internal_dispatch {
        prepare_internal_request(&mut req, dispatch)?;
    } else {
        telemetry::inject_context(req.headers_mut());
    }

    let response = client.request(req).await.map_err(|error| {
        tracing::error!("Error forwarding request to backend: {}", error);
        ErrorResponse::from(HttpError::BadGateway(
            "Failed to connect to backend service".to_string(),
        ))
    })?;

    let (parts, body) = response.into_parts();
    let mut upstream_headers = parts.headers;
    strip_hop_by_hop_headers(&mut upstream_headers);
    strip_internal_context_response(&mut upstream_headers);

    let mut proxied = Response::new(Body::new(body));
    *proxied.status_mut() = parts.status;
    *proxied.version_mut() = parts.version;
    *proxied.extensions_mut() = parts.extensions;
    *proxied.headers_mut() = upstream_headers;

    for (name, value) in headers {
        proxied.headers_mut().insert(name, value.clone());
    }

    Ok(proxied)
}

fn prepare_internal_request(
    req: &mut http::Request<Body>,
    dispatch: &InternalDispatch,
) -> Result<(), ErrorResponse> {
    sanitize_internal_request_headers(req.headers_mut(), dispatch.draft.request_id())
        .map_err(|_| internal_failure("sanitization"))?;

    // The active client span is already current because the executor
    // instruments this handler. Replace raw caller values before injecting.
    telemetry::inject_trace_context(req.headers_mut());
    let active_trace_id = telemetry::trace_id_from_span(&tracing::Span::current());
    if dispatch.draft.trace_id() != active_trace_id.as_deref() {
        return Err(internal_failure("trace"));
    }

    let issue_request = dispatch
        .draft
        .issue_request(
            &dispatch.audience,
            dispatch.kind,
            dispatch.attempt,
            req.method().as_str(),
            req.uri().path(),
        )
        .map_err(|_| internal_failure("binding"))?;
    let issued = dispatch
        .runtime
        .issue(&issue_request)
        .map_err(|error| internal_failure(issue_error_category(&error)))?;
    let value =
        http::HeaderValue::from_str(issued.compact()).map_err(|_| internal_failure("header"))?;

    req.headers_mut().remove(&INTERNAL_CONTEXT_HEADER);
    req.headers_mut().insert(&INTERNAL_CONTEXT_HEADER, value);
    telemetry::record_internal_context_issue("success", "none");
    Ok(())
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

fn strip_hop_by_hop_headers(headers: &mut http::HeaderMap) {
    let mut connection_headers = Vec::new();
    for value in headers.get_all(CONNECTION) {
        if let Ok(value) = value.to_str() {
            for header in value.split(',') {
                let header = header.trim();
                if header.is_empty() {
                    continue;
                }
                if let Ok(name) = http::header::HeaderName::from_bytes(header.as_bytes()) {
                    connection_headers.push(name);
                }
            }
        }
    }

    for name in connection_headers {
        headers.remove(name);
    }

    headers.remove(CONNECTION);
    headers.remove("keep-alive");
    headers.remove(PROXY_AUTHENTICATE);
    headers.remove(PROXY_AUTHORIZATION);
    headers.remove(TE);
    headers.remove(TRAILER);
    headers.remove(TRANSFER_ENCODING);
    headers.remove(UPGRADE);
    headers.remove("proxy-connection");
    headers.remove("trailers");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etc::{guard::VerifiedIdentity, reqctx::RequestContext};
    use axum::{Router, body::Body, extract::State, routing::any};
    use chrono::Utc;
    use ctx::{
        ActorType, ContextSigner, ContextVerifier, ExpectedRequest, SignerConfig,
        StaticKeyResolver, VerifierConfig,
    };
    use http::header::{AUTHORIZATION, CONNECTION, COOKIE, TE};
    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;
    use hyper_rustls::HttpsConnectorBuilder;
    use hyper_util::{
        client::legacy::{Client, connect::HttpConnector},
        rt::TokioExecutor,
    };
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;
    use tracing::Instrument;
    use tracing_subscriber::layer::SubscriberExt as _;

    const ISSUER: &str = "https://stargate.test/internal-context";
    const AUDIENCE: &str = "urn:stargate:service:orders";
    const KEY_ID: &str = "stargate-internal-test";
    const REQUEST_ID: &str = "01JZ000000000000000000000R";
    const PRIVATE_KEY: &[u8] = include_bytes!("../../../crates/ctx/tests/fixtures/private.pem");
    const PUBLIC_KEY: &[u8] = include_bytes!("../../../crates/ctx/tests/fixtures/public.pem");

    #[derive(Debug)]
    struct CapturedRequest {
        method: http::Method,
        uri: http::Uri,
        headers: http::HeaderMap,
    }

    #[derive(Clone)]
    struct CaptureState {
        sender: Arc<Mutex<Option<oneshot::Sender<CapturedRequest>>>>,
    }

    async fn capture_request(
        State(state): State<CaptureState>,
        req: http::Request<Body>,
    ) -> Response {
        let captured = CapturedRequest {
            method: req.method().clone(),
            uri: req.uri().clone(),
            headers: req.headers().clone(),
        };
        if let Some(sender) = state.sender.lock().unwrap().take() {
            let _ = sender.send(captured);
        }

        let mut response = Response::new(Body::empty());
        *response.status_mut() = http::StatusCode::NO_CONTENT;
        response.headers_mut().insert(
            INTERNAL_CONTEXT_HEADER.clone(),
            http::HeaderValue::from_static("must-not-escape"),
        );
        response
    }

    async fn mock_upstream() -> (
        String,
        oneshot::Receiver<CapturedRequest>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = oneshot::channel();
        let state = CaptureState {
            sender: Arc::new(Mutex::new(Some(sender))),
        };
        let app = Router::new()
            .fallback(any(capture_request))
            .with_state(state);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}"), receiver, server)
    }

    fn test_client() -> crate::etc::gate::HyperClient {
        crate::etc::tls::install_crypto_provider();
        let mut connector = HttpConnector::new();
        connector.enforce_http(false);
        let https = HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .wrap_connector(connector);
        Client::builder(TokioExecutor::new()).build(https)
    }

    fn test_runtime() -> Arc<InternalContextRuntime> {
        let signer = ContextSigner::from_rsa_pem(
            SignerConfig::new(ISSUER, KEY_ID, 30).unwrap(),
            PRIVATE_KEY,
        )
        .unwrap();
        InternalContextRuntime::from_signer_for_test(signer)
    }

    fn anonymous_draft(trace_id: Option<String>) -> Arc<PropagationDraft> {
        let request_context = RequestContext::new(
            REQUEST_ID.to_owned(),
            Utc::now(),
            Some("203.0.113.10".parse().unwrap()),
            Some("gateway-test/1.0".to_owned()),
            trace_id,
        );
        Arc::new(
            PropagationDraft::build(
                &VerifiedIdentity::anonymous(),
                &request_context,
                "POST",
                "/v1/orders",
                "/api/orders",
                "orders-write",
                "orders",
                Some("sha256:test".to_owned()),
            )
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn axum_body_wrapper_preserves_trailers() {
        let mut trailers = http::HeaderMap::new();
        trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
        trailers.insert("x-upstream-trailer", http::HeaderValue::from_static("ok"));

        let body = Full::new(Bytes::from_static(b"pong"))
            .with_trailers(async move { Some(Ok::<_, std::convert::Infallible>(trailers)) });

        let collected = Body::new(body).collect().await.unwrap();
        let trailers = collected.trailers().cloned().expect("missing trailers");
        assert_eq!(collected.to_bytes(), Bytes::from_static(b"pong"));
        assert_eq!(trailers["grpc-status"], "0");
        assert_eq!(trailers["x-upstream-trailer"], "ok");
    }

    #[test]
    fn strip_hop_by_hop_headers_removes_standard_and_connection_listed_headers() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            CONNECTION,
            http::HeaderValue::from_static("keep-alive, x-remove"),
        );
        headers.insert("keep-alive", http::HeaderValue::from_static("timeout=5"));
        headers.insert("x-remove", http::HeaderValue::from_static("1"));
        headers.insert(TE, http::HeaderValue::from_static("trailers"));
        headers.insert("x-keep", http::HeaderValue::from_static("ok"));

        strip_hop_by_hop_headers(&mut headers);

        assert!(headers.get(CONNECTION).is_none());
        assert!(headers.get("keep-alive").is_none());
        assert!(headers.get("x-remove").is_none());
        assert!(headers.get(TE).is_none());
        assert_eq!(headers.get("x-keep").unwrap(), "ok");
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

    #[tokio::test(flavor = "current_thread")]
    async fn internal_http_dispatch_is_signed_bound_sanitized_and_not_reflected() {
        let provider = SdkTracerProvider::builder().build();
        let tracer = provider.tracer("gateway-http-test");
        let subscriber =
            tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
        let guard = tracing::subscriber::set_default(subscriber);

        let server_span = tracing::info_span!("test.server", otel.kind = "server");
        let trace_id = telemetry::trace_id_from_span(&server_span).unwrap();
        let client_span = {
            let _entered = server_span.enter();
            tracing::info_span!("test.client", otel.kind = "client")
        };

        let draft = anonymous_draft(Some(trace_id.clone()));
        let dispatch = InternalDispatch::for_test(AUDIENCE, draft, test_runtime());
        let (base_url, captured, server) = mock_upstream().await;
        let mut req = http::Request::builder()
            .method("POST")
            .uri("/ignored")
            .header(AUTHORIZATION, "Bearer edge-secret")
            .header("proxy-authorization", "Basic edge-secret")
            .header("x-api-key", "edge-api-key")
            .header("baggage", "private=value")
            .header("x-request-id", "attacker-request-id")
            .header("stargate-context", "attacker-context")
            .header(
                "traceparent",
                "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01",
            )
            .header(COOKIE, "theme=dark; jwt=edge-session; cart=123")
            .body(Body::from("payload"))
            .unwrap();
        req.headers_mut().append(
            "x-request-id",
            http::HeaderValue::from_static("second-attacker-id"),
        );
        let target = format!("{base_url}/internal/v1/orders?keep=%2Fencoded&%74oken=secret&z=last");

        let response = handler(
            req,
            &http::HeaderMap::new(),
            &target,
            &test_client(),
            false,
            Some(&dispatch),
        )
        .instrument(client_span.clone())
        .await
        .unwrap();
        let captured = captured.await.unwrap();

        assert_eq!(response.status(), http::StatusCode::NO_CONTENT);
        assert!(response.headers().get(INTERNAL_CONTEXT_HEADER).is_none());
        assert_eq!(captured.method, http::Method::POST);
        assert_eq!(captured.uri.path(), "/internal/v1/orders");
        assert_eq!(captured.uri.query(), Some("keep=%2Fencoded&z=last"));
        for name in [
            AUTHORIZATION,
            http::header::PROXY_AUTHORIZATION,
            http::HeaderName::from_static("x-api-key"),
            http::HeaderName::from_static("baggage"),
        ] {
            assert!(captured.headers.get_all(name).iter().next().is_none());
        }
        assert_eq!(captured.headers[COOKIE], "theme=dark; cart=123");
        assert_eq!(captured.headers.get_all("x-request-id").iter().count(), 1);
        assert_eq!(captured.headers["x-request-id"], REQUEST_ID);
        assert_eq!(
            telemetry::trace_id_from_headers(&captured.headers).as_deref(),
            Some(trace_id.as_str())
        );

        let tokens = captured
            .headers
            .get_all(INTERNAL_CONTEXT_HEADER)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(tokens.len(), 1);
        let resolver = StaticKeyResolver::from_rsa_pem(KEY_ID, PUBLIC_KEY).unwrap();
        let verifier = ContextVerifier::new(
            VerifierConfig::new(ISSUER, AUDIENCE, 5).unwrap(),
            Arc::new(resolver),
        );
        let trusted = verifier
            .verify(
                tokens[0],
                ExpectedRequest {
                    method: "POST",
                    encoded_path: "/internal/v1/orders",
                    request_id: REQUEST_ID,
                },
            )
            .unwrap();
        assert_eq!(trusted.audience(), AUDIENCE);
        assert_eq!(trusted.context().actor.actor_type, ActorType::Anonymous);
        assert_eq!(
            trusted.context().request.trace_id.as_deref(),
            Some(trace_id.as_str())
        );
        assert_eq!(trusted.context().dispatch.kind, DispatchKind::Primary);
        assert_eq!(trusted.context().dispatch.attempt, 1);

        server.abort();
        drop(client_span);
        drop(server_span);
        drop(guard);
        provider.shutdown().unwrap();
    }

    #[tokio::test]
    async fn issuance_failure_contacts_no_upstream() {
        let (base_url, mut captured, server) = mock_upstream().await;
        let oversized_audience = "a".repeat(ctx::MAX_AUDIENCE_BYTES + 1);
        let dispatch =
            InternalDispatch::for_test(&oversized_audience, anonymous_draft(None), test_runtime());
        let req = http::Request::builder()
            .method("POST")
            .uri("/ignored")
            .body(Body::empty())
            .unwrap();

        let error = handler(
            req,
            &http::HeaderMap::new(),
            &format!("{base_url}/v1/orders"),
            &test_client(),
            false,
            Some(&dispatch),
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(matches!(
            captured.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        server.abort();
    }

    #[tokio::test]
    async fn absent_internal_block_preserves_existing_forwarding_behavior() {
        let (base_url, captured, server) = mock_upstream().await;
        let req = http::Request::builder()
            .method("GET")
            .uri("/ignored")
            .header(AUTHORIZATION, "Bearer still-forwarded")
            .header("x-api-key", "still-forwarded")
            .header(COOKIE, "jwt=still-forwarded")
            .body(Body::empty())
            .unwrap();

        let response = handler(
            req,
            &http::HeaderMap::new(),
            &format!("{base_url}/external?token=still-forwarded&keep=1"),
            &test_client(),
            false,
            None,
        )
        .await
        .unwrap();
        let captured = captured.await.unwrap();

        assert_eq!(captured.headers[AUTHORIZATION], "Bearer still-forwarded");
        assert_eq!(captured.headers["x-api-key"], "still-forwarded");
        assert_eq!(captured.headers[COOKIE], "jwt=still-forwarded");
        assert_eq!(captured.uri.query(), Some("token=still-forwarded&keep=1"));
        assert!(captured.headers.get(INTERNAL_CONTEXT_HEADER).is_none());
        assert!(response.headers().get(INTERNAL_CONTEXT_HEADER).is_none());
        server.abort();
    }
}
