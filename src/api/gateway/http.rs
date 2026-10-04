use super::{
    dispatch::InternalDispatch,
    headers::{strip_hop_by_hop_headers, strip_internal_context_response},
    lifecycle::{Execution, guarded_body},
};
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::telemetry;
use axum::{body::Body, response::Response};
use http::header::HOST;

pub async fn handler(
    mut req: http::Request<Body>,
    uri: &str,
    client: &crate::etc::gate::HyperClient,
    preserve_host: bool,
    internal_dispatch: Option<&InternalDispatch>,
    settings: &gate::cfg::CompiledRuntimeSettings,
    execution: &Execution,
) -> Result<Response, ErrorResponse> {
    req.extensions_mut()
        .remove::<std::sync::Arc<crate::etc::gate::resources::Admission>>();
    let upstream_uri = parse_upstream_uri(uri)?;
    if !matches!(
        req.version(),
        http::Version::HTTP_10 | http::Version::HTTP_11 | http::Version::HTTP_2
    ) || (req.version() == http::Version::HTTP_10 && req.method() == http::Method::CONNECT)
    {
        return Err(ErrorResponse::new(
            ErrorCode::GatewayRequestPreparationFailed,
        ));
    }
    // Hyper treats an HTTP/2 request version as a required egress protocol.
    // Let the prepared client's protocol policy and ALPN choose that hop.
    if req.version() == http::Version::HTTP_2 {
        *req.version_mut() = http::Version::HTTP_11;
    }
    *req.uri_mut() = upstream_uri;
    strip_hop_by_hop_headers(req.headers_mut());
    if !preserve_host {
        req.headers_mut().remove(HOST);
    }
    if let Some(dispatch) = internal_dispatch {
        dispatch.prepare(&mut req)?;
    } else {
        telemetry::inject_context(req.headers_mut());
    }

    let (parts, body) = req.into_parts();
    let req = http::Request::from_parts(
        parts,
        guarded_body(
            body,
            execution.clone(),
            settings.upload_idle_timeout,
            "upload",
            None,
        ),
    );
    let response = execution
        .run(
            "response_headers",
            settings.response_header_timeout,
            client.request(req),
        )
        .await?
        .map_err(|error| {
            if let Some(failure) = execution.failure() {
                return failure;
            }
            if crate::etc::gate::connection_timed_out(&error) {
                return execution.timeout("connect");
            }
            if request_preparation_failed(&error) {
                return ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed);
            }
            tracing::error!("Error forwarding request to backend: {}", error);
            ErrorResponse::new(ErrorCode::UpstreamConnectionFailed)
        })?;

    let (parts, body) = response.into_parts();
    let mut upstream_headers = parts.headers;
    strip_hop_by_hop_headers(&mut upstream_headers);
    strip_internal_context_response(&mut upstream_headers);

    let mut proxied = Response::new(guarded_body(
        Body::new(body),
        execution.response(),
        settings.response_body_idle_timeout,
        "response_body",
        None,
    ));
    *proxied.status_mut() = parts.status;
    *proxied.version_mut() = parts.version;
    *proxied.extensions_mut() = parts.extensions;
    *proxied.headers_mut() = upstream_headers;

    Ok(proxied)
}

fn request_preparation_failed(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut source = Some(error);
    while let Some(error) = source {
        if error
            .downcast_ref::<hyper::Error>()
            .is_some_and(hyper::Error::is_user)
        {
            return true;
        }
        source = error.source();
    }
    false
}

pub(super) fn parse_upstream_uri(uri: &str) -> Result<http::Uri, ErrorResponse> {
    let parsed = uri.parse::<http::Uri>().map_err(|error| {
        tracing::error!(%error, "Invalid upstream URI");
        ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed)
    })?;
    if !matches!(parsed.scheme_str(), Some("http" | "https")) || parsed.authority().is_none() {
        return Err(ErrorResponse::new(
            ErrorCode::GatewayRequestPreparationFailed,
        ));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etc::{
        guard::VerifiedIdentity,
        internal_context::InternalContextRuntime,
        reqctx::{INTERNAL_CONTEXT_HEADER, PropagationDraft, RequestContext},
    };
    use axum::{Router, body::Body, extract::State, routing::any};
    use chrono::Utc;
    use ctx::{
        ActorType, ContextSigner, ContextVerifier, DispatchKind, ExpectedRequest, SignerConfig,
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
        version: http::Version,
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
            version: req.version(),
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
        Client::builder(TokioExecutor::new()).build(crate::etc::gate::DeadlineConnector::new(
            https,
            std::time::Duration::from_secs(5),
        ))
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

    #[tokio::test]
    async fn downstream_http2_does_not_override_an_http1_upstream_policy() {
        let (url, captured, server) = mock_upstream().await;
        let mut value = serde_json::to_value(gate::cfg::Config::default()).unwrap();
        value["http"] = serde_json::json!({
            "upstreams":{"orders":{"targets":[{"url":url}],"transport":{"protocols":["http1"]}}},
            "services":{"orders":{"kind":"load_balancer","upstream":"orders"}}
        });
        let runtime = crate::etc::gate::test_support::runtime(
            gate::cfg::RuntimeConfig::from_raw(serde_json::from_value(value).unwrap()).unwrap(),
        );
        let req = http::Request::builder()
            .version(http::Version::HTTP_2)
            .uri("/orders")
            .body(Body::empty())
            .unwrap();
        let response = handler(
            req,
            &url,
            &runtime.client("orders").unwrap(),
            false,
            None,
            &runtime.settings,
            &Execution::default(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), http::StatusCode::NO_CONTENT);
        assert_eq!(captured.await.unwrap().version, http::Version::HTTP_11);
        server.abort();
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
        let runtime = test_runtime();
        let dispatch = InternalDispatch::new(
            "orders",
            AUDIENCE,
            Some(&draft),
            Some(&runtime),
            DispatchKind::Primary,
            1,
        )
        .unwrap();
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
            &target,
            &test_client(),
            false,
            Some(&dispatch),
            &gate::cfg::RuntimeSettings::default().compile().unwrap(),
            &Execution::default(),
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
        let draft = anonymous_draft(None);
        let runtime = test_runtime();
        let dispatch = InternalDispatch::new(
            "orders",
            &oversized_audience,
            Some(&draft),
            Some(&runtime),
            DispatchKind::Primary,
            1,
        )
        .unwrap();
        let req = http::Request::builder()
            .method("POST")
            .uri("/ignored")
            .body(Body::empty())
            .unwrap();

        let error = handler(
            req,
            &format!("{base_url}/v1/orders"),
            &test_client(),
            false,
            Some(&dispatch),
            &gate::cfg::RuntimeSettings::default().compile().unwrap(),
            &Execution::default(),
        )
        .await
        .unwrap_err();

        assert_eq!(error.status, http::StatusCode::INTERNAL_SERVER_ERROR);
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
            &format!("{base_url}/external?token=still-forwarded&keep=1"),
            &test_client(),
            false,
            None,
            &gate::cfg::RuntimeSettings::default().compile().unwrap(),
            &Execution::default(),
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
