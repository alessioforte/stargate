use crate::etc::ext::RequestExt;
use actix_web::HttpMessage;
use db::ent::AuditContext;
use tracing::info;
use tracing_actix_web::{DefaultRootSpanBuilder, RootSpanBuilder};
use tracing_subscriber::{
    EnvFilter, Registry,
    fmt::{self, format::FmtSpan, time::UtcTime},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};
use ulid::Ulid;

pub fn init() -> tracing_appender::non_blocking::WorkerGuard {
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| ".stargate/logs".to_string());
    let file_appender = tracing_appender::rolling::daily(log_dir.clone(), "stargate.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::from_default_env();

    // JSON file layer for structured logs
    let file_layer = fmt::layer()
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
        .with_ansi(false);

    // Console layer for development
    let console_layer = fmt::layer()
        .with_timer(UtcTime::rfc_3339())
        .with_span_events(FmtSpan::CLOSE)
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_target(true)
        .with_level(true)
        .with_ansi(true)
        .pretty();

    Registry::default()
        .with(env_filter)
        .with(file_layer)
        .with(console_layer)
        .init();

    info!("Logging initialized. Logs will be written to {}", log_dir);

    _guard
}

pub struct StargateRootSpanBuilder;

impl RootSpanBuilder for StargateRootSpanBuilder {
    fn on_request_start(sr: &actix_web::dev::ServiceRequest) -> tracing::Span {
        let req = sr.request();

        let request_id = Ulid::new().to_string();
        let user_agent = req
            .get_user_agent()
            .unwrap_or_else(|| "unknown".to_string());
        let method = req.method().as_str();
        let path = req.path();
        let query = req.query_string();
        let peer_ip = req
            .connection_info()
            .peer_addr()
            .unwrap_or("unknown")
            .to_string();

        let ip_address = match req.get_client_ip() {
            Some(ip) => ip.to_string(),
            None => "unknown".to_string(),
        };

        // Initialize the audit context with the request context
        // this is a bridge between the tracing context and the audit context
        let ctx = AuditContext::anonymous().with_request_context(
            request_id.clone(),
            ip_address.clone(),
            user_agent.clone(),
        );
        req.extensions_mut().insert(ctx);

        tracing::info_span!("http_request",
            request_id = %request_id,
            http.method = %method,
            http.path = %path,
            http.query = %query,
            http.user_agent = %user_agent,
            http.peer_ip = %peer_ip,
            http.ip_address = %ip_address,
            http.status_code = tracing::field::Empty,
            // http.latency_ms = tracing::field::Empty,
        )
    }

    fn on_request_end<B: awc::body::MessageBody>(
        span: tracing::Span,
        outcome: &Result<actix_web::dev::ServiceResponse<B>, actix_web::Error>,
    ) {
        match outcome {
            Ok(response) => {
                let status_code = response.status().as_u16();
                span.record("http.status_code", status_code);
            }
            Err(error) => {
                tracing::error!(parent: &span, %error, "Request failed");
            }
        };

        DefaultRootSpanBuilder::on_request_end(span, outcome);
    }
}
