use crate::etc::{
    env::bool_or,
    ext::RequestExt,
    reqctx::{self, RequestContext},
    telemetry,
};
use chrono::Utc;
use std::time::Instant;
use tracing::info;
use tracing_subscriber::{
    EnvFilter, Registry,
    fmt::{self, format::FmtSpan, time::UtcTime},
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
};
use ulid::Ulid;

pub struct LogGuard {
    _guards: Vec<tracing_appender::non_blocking::WorkerGuard>,
    telemetry: telemetry::TelemetryGuard,
}

impl LogGuard {
    pub fn shutdown(&self) {
        self.telemetry.shutdown();
    }
}

pub fn init() -> LogGuard {
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| ".stargate/logs".to_string());
    let env_filter = EnvFilter::from_default_env();
    let mut file_enabled = bool_or("LOG_FILE_ENABLED", true);
    let mut console_enabled = bool_or("LOG_CONSOLE_ENABLED", cfg!(debug_assertions));
    let pretty_console = bool_or("LOG_PRETTY_CONSOLE", cfg!(debug_assertions));
    let mut guards = Vec::new();
    let telemetry_guard = telemetry::init();

    if !file_enabled && !console_enabled {
        console_enabled = true;
        file_enabled = false;
    }

    let file_layer = if file_enabled {
        let file_appender = tracing_appender::rolling::daily(log_dir.clone(), "stargate.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        guards.push(guard);
        Some(
            fmt::layer()
                .json()
                .with_writer(non_blocking)
                .with_timer(UtcTime::rfc_3339())
                .with_span_events(fmt::format::FmtSpan::CLOSE)
                .with_file(true)
                .with_line_number(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_target(true)
                .with_level(true)
                .with_ansi(false)
                .boxed(),
        )
    } else {
        None
    };

    let console_layer = if console_enabled {
        let base = fmt::layer()
            .with_timer(UtcTime::rfc_3339())
            .with_span_events(FmtSpan::CLOSE)
            .with_file(true)
            .with_line_number(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_target(true)
            .with_level(true)
            .with_ansi(true);
        if pretty_console {
            Some(base.pretty().boxed())
        } else {
            Some(base.compact().boxed())
        }
    } else {
        None
    };

    let trace_layer = telemetry_guard.tracer_provider().map(|provider| {
        use opentelemetry::trace::TracerProvider as _;

        let tracer = provider.tracer("stargate");
        tracing_opentelemetry::layer().with_tracer(tracer).boxed()
    });

    let telemetry_log_layer = telemetry_guard.logger_provider().map(|provider| {
        let filter = EnvFilter::new("trace")
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("opentelemetry=off".parse().unwrap())
            .add_directive("opentelemetry_otlp=off".parse().unwrap())
            .add_directive("opentelemetry_sdk=off".parse().unwrap())
            .add_directive("tonic=off".parse().unwrap())
            .add_directive("h2=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap());
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(provider)
            .with_filter(filter)
            .boxed()
    });

    Registry::default()
        .with(env_filter)
        .with(file_layer)
        .with(console_layer)
        .with(trace_layer)
        .with(telemetry_log_layer)
        .init();

    info!(
        log_dir,
        file_enabled,
        console_enabled,
        pretty_console = console_enabled && pretty_console,
        otel_enabled = telemetry_guard.enabled(),
        "Logging initialized"
    );

    LogGuard {
        _guards: guards,
        telemetry: telemetry_guard,
    }
}

const SENSITIVE_PARAMS: &[&str] = &["token", "code", "secret", "key", "password", "state"];

fn sanitize_query(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }
    query
        .split('&')
        .map(|pair| {
            if let Some((key, _)) = pair.split_once('=') {
                let lower = key.to_lowercase();
                if SENSITIVE_PARAMS.iter().any(|s| lower.contains(s)) {
                    return format!("{}=[REDACTED]", key);
                }
            }
            pair.to_string()
        })
        .collect::<Vec<_>>()
        .join("&")
}

pub async fn trace_middleware(
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use tracing::Instrument;

    let request_id = Ulid::generate().to_string();
    let now = Utc::now();
    let started = Instant::now();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = sanitize_query(req.uri().query().unwrap_or(""));
    let user_agent = req.get_user_agent();
    reqctx::sanitize_ingress_headers(req.headers_mut(), &request_id)
        .expect("generated request id must be a valid header value");
    let peer_ip = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let client_ip_addr = req.get_client_ip_addr();
    let ip_address = client_ip_addr
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let span = tracing::info_span!(
        "http_request",
        request_id = %request_id,
        http.method = %method,
        http.path = %path,
        http.query = %query,
        http.user_agent = %user_agent.as_deref().unwrap_or("unknown"),
        http.peer_ip = %peer_ip,
        http.client_ip = %ip_address,
        http.status_code = tracing::field::Empty,
        http.response.status_code = tracing::field::Empty,
        otel.kind = "server",
        otel.name = %format!("{} {}", method, path),
        otel.status_code = tracing::field::Empty,
        stargate.router = tracing::field::Empty,
        stargate.service = tracing::field::Empty,
        stargate.auth_kind = tracing::field::Empty,
        stargate.config_version = tracing::field::Empty,
    );

    telemetry::set_span_parent_from_headers(&span, req.headers());
    let trace_id = telemetry::trace_id_from_span(&span);
    req.extensions_mut().insert(RequestContext::new(
        request_id.clone(),
        now,
        client_ip_addr,
        user_agent.clone(),
        trace_id,
    ));

    let mut response = next.run(req).instrument(span.clone()).await;
    span.record("http.status_code", response.status().as_u16());
    span.record("http.response.status_code", response.status().as_u16());
    if response.status().is_server_error() {
        span.record("otel.status_code", "ERROR");
    }
    telemetry::record_http_server_request(
        method.as_str(),
        response.status().as_u16(),
        started.elapsed(),
    );
    response.headers_mut().insert(
        http::header::HeaderName::from_static("x-request-id"),
        request_id.parse().unwrap(),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, middleware::from_fn, routing::get};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

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
}
