mod metrics;
mod propagation;

use crate::etc::env::bool_or;
use opentelemetry::{KeyValue, global, propagation::TextMapCompositePropagator};
use opentelemetry_sdk::{
    Resource,
    logs::SdkLoggerProvider,
    metrics::SdkMeterProvider,
    propagation::{BaggagePropagator, TraceContextPropagator},
    trace::SdkTracerProvider,
};

const INSTRUMENTATION_NAME: &str = "stargate";

pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl TelemetryGuard {
    pub fn disabled() -> Self {
        Self {
            tracer_provider: None,
            meter_provider: None,
            logger_provider: None,
        }
    }

    pub fn enabled(&self) -> bool {
        self.tracer_provider.is_some()
            || self.meter_provider.is_some()
            || self.logger_provider.is_some()
    }

    pub fn tracer_provider(&self) -> Option<&SdkTracerProvider> {
        self.tracer_provider.as_ref()
    }

    pub fn logger_provider(&self) -> Option<&SdkLoggerProvider> {
        self.logger_provider.as_ref()
    }

    pub fn shutdown(&self) {
        if let Some(provider) = &self.tracer_provider
            && let Err(error) = provider.shutdown()
        {
            tracing::warn!(%error, "OpenTelemetry tracer provider shutdown failed");
        }
        if let Some(provider) = &self.meter_provider
            && let Err(error) = provider.shutdown()
        {
            tracing::warn!(%error, "OpenTelemetry meter provider shutdown failed");
        }
        if let Some(provider) = &self.logger_provider
            && let Err(error) = provider.shutdown()
        {
            tracing::warn!(%error, "OpenTelemetry logger provider shutdown failed");
        }
    }
}

pub fn init() -> TelemetryGuard {
    if !otel_enabled() {
        return TelemetryGuard::disabled();
    }

    global::set_text_map_propagator(TextMapCompositePropagator::new(vec![
        Box::new(TraceContextPropagator::new()),
        Box::new(BaggagePropagator::new()),
    ]));

    let resource = resource();

    let tracer_provider = match opentelemetry_otlp::SpanExporter::builder().build() {
        Ok(exporter) => {
            let provider = SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource.clone())
                .build();
            global::set_tracer_provider(provider.clone());
            Some(provider)
        }
        Err(error) => {
            tracing::warn!(%error, "OpenTelemetry trace exporter disabled");
            None
        }
    };

    let meter_provider = match opentelemetry_otlp::MetricExporter::builder().build() {
        Ok(exporter) => {
            let provider = SdkMeterProvider::builder()
                .with_periodic_exporter(exporter)
                .with_resource(resource.clone())
                .build();
            global::set_meter_provider(provider.clone());
            Some(provider)
        }
        Err(error) => {
            tracing::warn!(%error, "OpenTelemetry metric exporter disabled");
            None
        }
    };

    let logger_provider = match opentelemetry_otlp::LogExporter::builder().build() {
        Ok(exporter) => Some(
            SdkLoggerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource)
                .build(),
        ),
        Err(error) => {
            tracing::warn!(%error, "OpenTelemetry log exporter disabled");
            None
        }
    };

    TelemetryGuard {
        tracer_provider,
        meter_provider,
        logger_provider,
    }
}

fn otel_enabled() -> bool {
    let default = [
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
        "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
        "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
    ]
    .iter()
    .any(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()));

    bool_or("OTEL_ENABLED", default)
}

fn resource() -> Resource {
    let service_name =
        std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "stargate".to_string());

    Resource::builder()
        .with_service_name(service_name)
        .with_attributes([
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new(
                "deployment.profile",
                crate::etc::server::profile::COMPILED_PROFILE,
            ),
            KeyValue::new(
                "db.system",
                crate::etc::server::profile::COMPILED_DB_BACKEND,
            ),
            KeyValue::new(
                "stargate.state_backend",
                crate::etc::server::profile::COMPILED_STATE_BACKEND,
            ),
        ])
        .build()
}

#[cfg(all(feature = "postgres", feature = "redis"))]
pub use metrics::record_audit_relay_batch;
pub use metrics::{
    record_config_reload, record_gateway_failover, record_gateway_mirror, record_gateway_policy,
    record_gateway_rejection, record_gateway_replay_bytes, record_gateway_request,
    record_gateway_resource, record_gateway_response_disposal, record_gateway_timeout,
    record_gateway_transfer, record_gateway_upstream_attempt, record_http_server_request,
    record_internal_context_issue, record_internal_context_queue, record_propagation_draft,
};
#[cfg(test)]
pub use propagation::trace_id_from_headers;
pub use propagation::{
    inject_context, inject_trace_context, set_span_parent_from_headers, trace_id_from_span,
};
