//! HTTP server semantic conventions v1.45.0:
//! https://github.com/open-telemetry/semantic-conventions/tree/v1.45.0/docs/http
use opentelemetry::{KeyValue, trace::TraceContextExt};
use std::sync::{Arc, OnceLock};
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// RFC 9110, PATCH and QUERY; unknown methods never become metric dimensions.
pub(crate) fn normalized_method(method: &str) -> &'static str {
    match method {
        "GET" => "GET",
        "HEAD" => "HEAD",
        "POST" => "POST",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        "CONNECT" => "CONNECT",
        "OPTIONS" => "OPTIONS",
        "TRACE" => "TRACE",
        "PATCH" => "PATCH",
        "QUERY" => "QUERY",
        _ => "_OTHER",
    }
}
pub(crate) fn operation_method(method: &'static str) -> &'static str {
    if method == "_OTHER" { "HTTP" } else { method }
}

/// Keep tracing's JSON fields and the live OpenTelemetry span name in sync.
/// In tracing-opentelemetry 0.34, after first entry, record updates SDK status
/// and attributes but not its name (SpanBuilderUpdates::update_span). The SDK
/// update is therefore required even when otel.name was successfully recorded.
fn set_span_name(span: &tracing::Span, name: &str) {
    span.record("otel.name", name);
    span.context().span().update_name(name.to_owned());
}

#[derive(Clone)]
pub(crate) struct HttpRequestTelemetry {
    span: tracing::Span,
    method: &'static str,
    route: Arc<OnceLock<String>>,
    error: Arc<OnceLock<String>>,
}
impl HttpRequestTelemetry {
    pub(crate) fn new(span: tracing::Span, method: &'static str) -> Self {
        Self {
            span,
            method,
            route: Arc::default(),
            error: Arc::default(),
        }
    }
    pub(crate) fn record_route(&self, route: &str) {
        if self.route.set(route.to_owned()).is_ok() {
            self.span.record("http.route", route);
            set_span_name(
                &self.span,
                &format!("{} {route}", operation_method(self.method)),
            );
        }
    }
    pub(crate) fn record_gateway(&self, router: &str, service: &str, version: u64) {
        self.span.record("stargate.router", router);
        self.span.record("stargate.service", service);
        self.span.record("stargate.config_version", version);
    }
    pub(crate) fn record_error(&self, error: &str) {
        if self.error.set(error.to_owned()).is_ok() {
            self.span.record("error.type", error);
            self.span.record("otel.status_code", "ERROR");
        }
    }
    pub(crate) fn metric_attributes(
        &self,
        kind: super::HttpRequestKind,
        status: u16,
        scheme: &'static str,
    ) -> Vec<KeyValue> {
        let mut attrs = vec![
            KeyValue::new("http.request.method", self.method),
            KeyValue::new("http.response.status_code", i64::from(status)),
            KeyValue::new("url.scheme", scheme),
            KeyValue::new("stargate.request.kind", kind.as_str()),
        ];
        if let Some(route) = self.route.get() {
            attrs.push(KeyValue::new("http.route", route.clone()));
        }
        if let Some(error) = self.error.get() {
            attrs.push(KeyValue::new("error.type", error.clone()));
        }
        attrs
    }
}

#[cfg(test)]
mod tests;
