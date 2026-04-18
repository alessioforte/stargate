use crate::etc::{ext::RequestExt, reqctx::RequestContext};
use actix_web::HttpMessage;
use chrono::{DateTime, Utc};
use tracing::info;
use tracing_actix_web::{DefaultRootSpanBuilder, RootSpanBuilder};
use tracing_subscriber::{
    EnvFilter, Registry,
    fmt::{self, format::FmtSpan, time::UtcTime},
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
};
use ulid::Ulid;

fn parse_bool_env(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        },
        Err(_) => default,
    }
}

pub fn init() -> Vec<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| ".stargate/logs".to_string());
    let env_filter = EnvFilter::from_default_env();
    let mut file_enabled = parse_bool_env("LOG_FILE_ENABLED", true);
    let mut console_enabled = parse_bool_env("LOG_CONSOLE_ENABLED", cfg!(debug_assertions));
    let pretty_console = parse_bool_env("LOG_PRETTY_CONSOLE", cfg!(debug_assertions));
    let mut guards = Vec::new();

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

    Registry::default()
        .with(env_filter)
        .with(file_layer)
        .with(console_layer)
        .init();

    info!(
        log_dir,
        file_enabled,
        console_enabled,
        pretty_console = console_enabled && pretty_console,
        "Logging initialized"
    );

    guards
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

pub struct StargateRootSpanBuilder;

impl RootSpanBuilder for StargateRootSpanBuilder {
    fn on_request_start(sr: &actix_web::dev::ServiceRequest) -> tracing::Span {
        let request_id = Ulid::new().to_string();

        let req = sr.request();
        let gate = req
            .app_data::<actix_web::web::Data<gate::Gate>>()
            .expect("Gate app_data must be configured in HttpServer");
        let ts = gate.clock.now_millis() as i64;
        let now: DateTime<Utc> = DateTime::from_timestamp_millis(ts).unwrap_or_else(|| Utc::now());
        let user_agent = req.get_user_agent();
        let method = req.method().as_str();
        let path = req.path();
        let query = sanitize_query(req.query_string());
        let peer_ip = req
            .connection_info()
            .peer_addr()
            .unwrap_or("unknown")
            .to_string();
        let ip_addr = req.get_client_ip_addr();
        let ip_address = ip_addr
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        req.extensions_mut().insert(RequestContext::new(
            request_id.clone(),
            now,
            ip_addr,
            user_agent.clone(),
        ));

        tracing::info_span!("http_request",
            request_id = %request_id,
            http.method = %method,
            http.path = %path,
            http.query = %query,
            http.user_agent = %user_agent.as_deref().unwrap_or("unknown"),
            http.peer_ip = %peer_ip,
            http.client_ip = %ip_address,
            http.status_code = tracing::field::Empty,
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
