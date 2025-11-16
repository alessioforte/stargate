use tracing::info;
use tracing_subscriber::{
    EnvFilter, Layer,
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

pub fn init() {
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| ".stargate/logs".to_string());
    let file_appender = tracing_appender::rolling::daily(log_dir.clone(), "stargate.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::from_default_env();

    // JSON file layer for structured logs
    let file_layer = fmt::layer()
        .json()
        .with_writer(non_blocking)
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_filter(env_filter.clone());

    // Console layer for development
    let console_layer = fmt::layer()
        .pretty()
        .with_span_events(FmtSpan::CLOSE)
        .with_target(true)
        .with_filter(env_filter);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .init();

    info!("Logging initialized. Logs will be written to {}", log_dir);
}
