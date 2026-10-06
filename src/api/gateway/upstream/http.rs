use crate::api::gateway::{
    lifecycle::{Execution, body::guarded_body},
    response::strip_internal_context_response,
    upstream::{headers::strip_hop_by_hop_headers, internal_context::InternalDispatch},
};
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::observability::telemetry;
use axum::{body::Body, response::Response};
use http::header::HOST;

pub async fn handler(
    mut req: http::Request<Body>,
    uri: &str,
    client: &crate::etc::gate::HyperClient,
    preserve_host: bool,
    internal_dispatch: Option<&InternalDispatch>,
    settings: &gate::cfg::CompiledRuntimeSettings,
    execution: &Execution,
) -> Result<Response, ErrorResponse> {
    req.extensions_mut()
        .remove::<std::sync::Arc<crate::etc::gate::resources::Admission>>();
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
    if !preserve_host {
        req.headers_mut().remove(HOST);
    }
    if let Some(dispatch) = internal_dispatch {
        dispatch.prepare(&mut req).await?;
    } else {
        telemetry::inject_context(req.headers_mut());
    }

    let (parts, body) = req.into_parts();
    let req = http::Request::from_parts(
        parts,
        guarded_body(
            body,
            execution.clone(),
            settings.upload_idle_timeout,
            "upload",
            None,
        ),
    );
    let response = execution
        .run(
            "response_headers",
            settings.response_header_timeout,
            client.request(req),
        )
        .await?
        .map_err(|error| {
            if let Some(failure) = execution.failure() {
                return failure;
            }
            if crate::etc::gate::connection_timed_out(&error) {
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
        settings.response_body_idle_timeout,
        "response_body",
        None,
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

pub(in crate::api::gateway) fn parse_upstream_uri(uri: &str) -> Result<http::Uri, ErrorResponse> {
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
