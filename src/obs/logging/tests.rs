use super::*;
use axum::{Router, body::Body, middleware::from_fn, routing::get};
use http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn nested_request_logs_keep_request_id_without_an_otel_trace_id() {
    use std::{
        io::Write,
        sync::{Arc, Mutex},
    };
    use tracing::{Instrument, instrument::WithSubscriber};

    #[derive(Clone)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    async fn handler(req: axum::extract::Request) -> StatusCode {
        let context = req.extensions().get::<RequestContext>().unwrap();
        assert!(context.trace_id().is_none());
        async {
            tracing::info!("nested_request_log");
        }
        .instrument(tracing::info_span!("nested"))
        .await;
        StatusCode::NO_CONTENT
    }

    let buffer = Arc::new(Mutex::new(Vec::new()));
    let output = buffer.clone();
    let subscriber = tracing_subscriber::registry().with(
        fmt::layer()
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .with_ansi(false)
            .with_writer(move || Capture(output.clone())),
    );
    let app = Router::new()
        .route("/", get(handler))
        .layer(from_fn(trace_middleware));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(
                    "traceparent",
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .with_subscriber(subscriber)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let text = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
    assert!(!text.contains('\u{1b}'));
    let logs: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let nested = logs
        .iter()
        .find(|log| log["fields"]["message"] == "nested_request_log")
        .unwrap();
    assert_eq!(nested["span"]["name"], "nested");
    let request_span = nested["spans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|span| span["name"] == "http_request")
        .unwrap();
    assert_eq!(
        request_span["request_id"].as_str().unwrap(),
        response.headers()["x-request-id"]
    );
    assert!(request_span.get("trace_id").is_none());
}

async fn assert_sanitized(req: axum::extract::Request) -> StatusCode {
    assert!(
        req.headers()
            .get_all("stargate-context")
            .iter()
            .next()
            .is_none()
    );
    let request_ids = req
        .headers()
        .get_all("x-request-id")
        .iter()
        .collect::<Vec<_>>();
    assert_eq!(request_ids.len(), 1);
    let context = req.extensions().get::<RequestContext>().unwrap();
    assert_eq!(request_ids[0], context.request_id());
    StatusCode::NO_CONTENT
}

#[tokio::test]
async fn trace_boundary_sanitizes_gateway_owned_headers_before_handler() {
    let app = Router::new()
        .route("/", get(assert_sanitized))
        .layer(from_fn(trace_middleware));
    let request = Request::builder()
        .uri("/")
        .header("stargate-context", "attacker-one")
        .header("stargate-context", "attacker-two")
        .header("x-request-id", "attacker-one")
        .header("x-request-id", "attacker-two")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(response.headers().get_all("x-request-id").iter().count(), 1);
}
