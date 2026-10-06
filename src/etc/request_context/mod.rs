mod adapters;
pub mod geoip;
mod propagation;

pub use adapters::{
    audit_request_from, build_env_from, request_id_from, take_trusted_audit_context_from,
};
use chrono::{DateTime, Utc};
use http::header::{HeaderName, HeaderValue};
pub use propagation::PropagationDraft;
use std::{
    net::IpAddr,
    sync::{Arc, OnceLock},
};

pub const INTERNAL_CONTEXT_HEADER: HeaderName = HeaderName::from_static("stargate-context");
pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Clone)]
pub struct RequestContext {
    request_id: Box<str>,
    started_at: DateTime<Utc>,
    client_ip: Option<IpAddr>,
    user_agent: Option<Box<str>>,
    trace_id: Option<Box<str>>,
    geo: OnceLock<Arc<geoip::GeoInfo>>,
}

impl RequestContext {
    pub fn new(
        request_id: String,
        started_at: DateTime<Utc>,
        client_ip: Option<IpAddr>,
        user_agent: Option<String>,
        trace_id: Option<String>,
    ) -> Self {
        Self {
            request_id: request_id.into_boxed_str(),
            started_at,
            client_ip,
            user_agent: user_agent.map(String::into_boxed_str),
            trace_id: trace_id.map(String::into_boxed_str),
            geo: OnceLock::new(),
        }
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn client_ip(&self) -> Option<IpAddr> {
        self.client_ip
    }

    pub fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }

    pub fn trace_id(&self) -> Option<&str> {
        self.trace_id.as_deref()
    }

    fn geo(&self) -> &Arc<geoip::GeoInfo> {
        self.geo.get_or_init(|| {
            self.client_ip
                .and_then(geoip::lookup)
                .unwrap_or_else(geoip::GeoInfo::unknown)
        })
    }
}

/// Remove untrusted gateway-owned context and install the request id generated
/// for this ingress request before routing or configurable middleware runs.
pub fn sanitize_ingress_headers(
    headers: &mut http::HeaderMap,
    request_id: &str,
) -> Result<(), http::header::InvalidHeaderValue> {
    headers.remove(&INTERNAL_CONTEXT_HEADER);
    headers.remove(&REQUEST_ID_HEADER);
    headers.insert(&REQUEST_ID_HEADER, HeaderValue::from_str(request_id)?);
    Ok(())
}

#[cfg(test)]
mod tests;
