use super::UpstreamRequest;
use crate::err::{ErrorCode, ErrorResponse};
use crate::gate::{
    lifecycle::{
        Execution,
        body::{Termination, guarded_body},
    },
    response::strip_internal_context_response,
    upstream::{
        headers::{
            UpstreamHeaders, apply_forwarded_headers, strip_credentials, strip_hop_by_hop_headers,
        },
        uri::{QueryMode, sanitize_auth_uri},
    },
};
use crate::obs::telemetry;
use axum::{body::Body, response::Response};
use http::header::HOST;

pub async fn handler(
    mut req: http::Request<Body>,
    upstream: UpstreamRequest<'_>,
    client: &crate::gate::HyperClient,
    ingress: &UpstreamHeaders,
    settings: &gate::cfg::CompiledRuntimeSettings,
    execution: &Execution,
) -> Result<Response, ErrorResponse> {
    let UpstreamRequest {
        uri,
        internal_dispatch,
        forward_credentials,
    } = upstream;
    req.extensions_mut()
        .remove::<std::sync::Arc<crate::gate::resources::Admission>>();
    let upstream_uri = parse_upstream_uri(uri)?;
    if !matches!(
        req.version(),
        http::Version::HTTP_10 | http::Version::HTTP_11 | http::Version::HTTP_2
    ) || (req.version() == http::Version::HTTP_10 && req.method() == http::Method::CONNECT)
    {
        return Err(ErrorResponse::new(
            ErrorCode::GatewayRequestPreparationFailed,
        ));
    }
    // Hyper treats an HTTP/2 request version as a required egress protocol.
    // Let the prepared client's protocol policy and ALPN choose that hop.
    if req.version() == http::Version::HTTP_2 {
        *req.version_mut() = http::Version::HTTP_11;
    }
    *req.uri_mut() = upstream_uri;
    strip_hop_by_hop_headers(req.headers_mut());
    apply_forwarded_headers(req.headers_mut(), ingress);
    if !ingress.preserve_host {
        req.headers_mut().remove(HOST);
    }
    if let Some(dispatch) = internal_dispatch {
        dispatch.prepare(&mut req).await?;
    } else {
        if !forward_credentials {
            *req.uri_mut() = sanitize_auth_uri(req.uri(), QueryMode::Tolerant)
                .map_err(|_| ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed))?;
            strip_credentials(req.headers_mut());
        }
        telemetry::inject_context(req.headers_mut());
    }

    if execution.grpc {
        crate::gate::grpc::forward_timeout(req.headers_mut(), execution)?;
    }

    let (parts, body) = req.into_parts();
    let req = http::Request::from_parts(
        parts,
        guarded_body(
            body,
            execution.clone(),
            execution.upload_idle(settings.upload_idle_timeout),
            "upload",
            None,
            Termination::Error,
        ),
    );
    let response = execution
        .run(
            "response_headers",
            Some(settings.response_header_timeout),
            client.request(req),
        )
        .await?
        .map_err(|error| {
            if let Some(failure) = execution.failure() {
                return failure;
            }
            if crate::gate::connection_timed_out(&error) {
                return execution.timeout("connect");
            }
            if request_preparation_failed(&error) {
                return ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed);
            }
            tracing::error!("Error forwarding request to backend: {}", error);
            ErrorResponse::new(ErrorCode::UpstreamConnectionFailed)
        })?;

    let (parts, body) = response.into_parts();
    let mut upstream_headers = parts.headers;
    strip_hop_by_hop_headers(&mut upstream_headers);
    strip_internal_context_response(&mut upstream_headers);

    let mut proxied = Response::new(guarded_body(
        Body::new(body),
        execution.response(),
        Some(settings.response_body_idle_timeout),
        "response_body",
        None,
        Termination::Error,
    ));
    *proxied.status_mut() = parts.status;
    *proxied.version_mut() = parts.version;
    *proxied.extensions_mut() = parts.extensions;
    *proxied.headers_mut() = upstream_headers;

    Ok(proxied)
}

fn request_preparation_failed(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut source = Some(error);
    while let Some(error) = source {
        if error
            .downcast_ref::<hyper::Error>()
            .is_some_and(hyper::Error::is_user)
        {
            return true;
        }
        source = error.source();
    }
    false
}

pub(in crate::gate) fn parse_upstream_uri(uri: &str) -> Result<http::Uri, ErrorResponse> {
    let parsed = uri.parse::<http::Uri>().map_err(|error| {
        tracing::error!(%error, "Invalid upstream URI");
        ErrorResponse::new(ErrorCode::GatewayRequestPreparationFailed)
    })?;
    if !matches!(parsed.scheme_str(), Some("http" | "https")) || parsed.authority().is_none() {
        return Err(ErrorResponse::new(
            ErrorCode::GatewayRequestPreparationFailed,
        ));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests;
