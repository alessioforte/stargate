use http::{HeaderMap, HeaderName, HeaderValue};
use opentelemetry::{
    Context, global,
    propagation::{Extractor, Injector, TextMapPropagator as _},
    trace::TraceContextExt,
};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing_opentelemetry::OpenTelemetrySpanExt;

fn extract_context(headers: &HeaderMap) -> Context {
    global::get_text_map_propagator(|propagator| propagator.extract(&HeaderExtractor(headers)))
}

/// Return only a syntactically valid W3C trace id. Audit request facts do not
/// copy the raw `traceparent` header.
#[cfg(test)]
pub fn trace_id_from_headers(headers: &HeaderMap) -> Option<String> {
    let context = TraceContextPropagator::new().extract(&HeaderExtractor(headers));
    let span = context.span();
    let span_context = span.span_context();
    span_context
        .is_valid()
        .then(|| span_context.trace_id().to_string())
}

pub fn set_span_parent_from_headers(span: &tracing::Span, headers: &HeaderMap) {
    let parent = extract_context(headers);
    if let Err(error) = span.set_parent(parent) {
        tracing::trace!(%error, "OpenTelemetry parent context was not attached to span");
    }
}

/// Return the trace id assigned to a tracing span by the OpenTelemetry layer.
/// This captures a newly-created server trace when no valid parent was sent.
pub fn trace_id_from_span(span: &tracing::Span) -> Option<String> {
    let context = span.context();
    let otel_span = context.span();
    let span_context = otel_span.span_context();
    span_context
        .is_valid()
        .then(|| span_context.trace_id().to_string())
}

pub fn inject_context(headers: &mut HeaderMap) {
    let context = tracing::Span::current().context();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&context, &mut HeaderInjector(headers));
    });
}

/// Replace any raw inbound W3C headers and inject only the active span's trace
/// context. This is deterministic even when no exporter/SDK span is active:
/// stale caller-authored trace headers are removed instead of leaking through.
pub fn inject_trace_context(headers: &mut HeaderMap) {
    headers.remove("traceparent");
    headers.remove("tracestate");
    let context = tracing::Span::current().context();
    TraceContextPropagator::new().inject_context(&context, &mut HeaderInjector(headers));
}

struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        let name = HeaderName::from_bytes(key.as_bytes()).ok()?;
        self.0.get(name).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(HeaderName::as_str).collect()
    }
}

struct HeaderInjector<'a>(&'a mut HeaderMap);

impl Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        let Ok(name) = HeaderName::from_bytes(key.as_bytes()) else {
            return;
        };
        let Ok(value) = HeaderValue::from_str(&value) else {
            return;
        };
        self.0.insert(name, value);
    }

    fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }
}

#[cfg(test)]
mod tests;
