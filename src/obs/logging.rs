use crate::{
    ctx::{RequestContext, sanitize_ingress_headers},
    obs::telemetry,
    web::request::RequestExt,
};
use chrono::Utc;
use std::{io::IsTerminal, time::Instant};
use tracing::info;
use tracing_subscriber::{
    EnvFilter, Registry,
    fmt::{self, format::FmtSpan, time::UtcTime},
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
};
use ulid::Ulid;

mod settings;
pub use settings::{LogFormat, Settings};

pub struct LogGuard {
    _guards: Vec<tracing_appender::non_blocking::WorkerGuard>,
    telemetry: telemetry::TelemetryGuard,
}

impl LogGuard {
    pub fn shutdown(&self) {
        self.telemetry.shutdown();
    }
}

pub fn init(settings: Settings) -> LogGuard {
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| ".stargate/logs".to_string());
    let env_filter = EnvFilter::from_default_env();
    let file_enabled = settings.file_enabled;
    let console_enabled = settings.console_enabled;
    let mut guards = Vec::new();
    let telemetry_guard = telemetry::init();

    let file_layer = if file_enabled {
        let file_appender = tracing_appender::rolling::daily(log_dir.clone(), "stargate.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        guards.push(guard);
        Some(
            fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
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
            .with_writer(std::io::stdout)
            .with_ansi(settings.format.ansi(std::io::stdout().is_terminal()));
        Some(match settings.format {
            LogFormat::Json => base
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .boxed(),
            LogFormat::Pretty => base.pretty().boxed(),
            LogFormat::Compact => base.compact().boxed(),
        })
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
        format = settings.format.as_str(),
        otel_enabled = telemetry_guard.enabled(),
        "Logging initialized"
    );

    LogGuard {
        _guards: guards,
        telemetry: telemetry_guard,
    }
}

pub async fn trace_middleware(
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use tracing::Instrument;

    let request_id = Ulid::generate().to_string();
    let now = Utc::now();
    let started = Instant::now();
    let method = telemetry::normalized_method(req.method().as_str());
    let original_method =
        (method != req.method().as_str()).then(|| req.method().as_str().to_owned());
    let path = req.uri().path().to_string();
    let scheme = if crate::srv::tls::served_over_https() {
        "https"
    } else {
        "http"
    };
    let authority = req.uri().authority().cloned().or_else(|| {
        req.headers()
            .get(http::header::HOST)?
            .to_str()
            .ok()?
            .parse::<http::uri::Authority>()
            .ok()
    });
    let server_address = authority
        .as_ref()
        .map(|authority| authority.host().trim_matches(['[', ']']).to_owned());
    let server_port = authority.as_ref().map(|authority| {
        authority
            .port_u16()
            .unwrap_or(if scheme == "https" { 443 } else { 80 })
    });
    let user_agent = req.get_user_agent();
    sanitize_ingress_headers(req.headers_mut(), &request_id)
        .expect("generated request id must be a valid header value");
    let peer_ip = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip().to_string());
    let client_ip_addr = req.get_client_ip_addr();
    let ip_address = client_ip_addr.map(|ip| ip.to_string());

    let span = tracing::info_span!(
        "http_request",
        request_id = %request_id,
        trace_id = tracing::field::Empty,
        // OpenTelemetry HTTP semantic conventions v1.45.0; query strings are omitted.
        http.request.method = method,
        http.request.method_original = original_method.as_deref(),
        http.route = tracing::field::Empty,
        url.path = %path,
        url.scheme = scheme,
        server.address = server_address.as_deref(),
        server.port = server_port.map(i64::from),
        user_agent.original = user_agent.as_deref(),
        network.peer.address = peer_ip.as_deref(),
        client.address = ip_address.as_deref(),
        http.response.status_code = tracing::field::Empty,
        error.type = tracing::field::Empty,
        otel.kind = "server",
        otel.name = telemetry::operation_method(method),
        otel.status_code = tracing::field::Empty,
        stargate.router = tracing::field::Empty,
        stargate.service = tracing::field::Empty,
        stargate.auth_kind = tracing::field::Empty,
        stargate.config_version = tracing::field::Empty,
    );

    telemetry::set_span_parent_from_headers(&span, req.headers());
    let trace_id = telemetry::trace_id_from_span(&span);
    if let Some(trace_id) = trace_id.as_deref() {
        span.record("trace_id", trace_id);
    }
    req.extensions_mut().insert(RequestContext::new(
        request_id.clone(),
        now,
        client_ip_addr,
        user_agent.clone(),
        trace_id,
    ));

    let request_telemetry = telemetry::HttpRequestTelemetry::new(span.clone(), method);
    req.extensions_mut().insert(request_telemetry.clone());
    let mut response = next.run(req).instrument(span.clone()).await;
    span.record("http.response.status_code", response.status().as_u16());
    if response.status().is_server_error() {
        request_telemetry.record_error(&response.status().as_u16().to_string());
    }
    telemetry::record_http_server_request(
        if response
            .extensions()
            .get::<telemetry::GatewayResponse>()
            .is_some()
        {
            telemetry::HttpRequestKind::Gateway
        } else {
            telemetry::HttpRequestKind::Iam
        },
        &request_telemetry,
        response.status().as_u16(),
        scheme,
        started.elapsed(),
    );
    response.headers_mut().insert(
        http::header::HeaderName::from_static("x-request-id"),
        request_id.parse().unwrap(),
    );
    response
}

#[cfg(test)]
mod tests;
