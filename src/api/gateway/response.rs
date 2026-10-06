use crate::api::gateway::upstream::headers::{append_header, insert_header};
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::request_context::INTERNAL_CONTEXT_HEADER;
use ::http::HeaderMap;
use ::http::{StatusCode, header::CONTENT_TYPE};
use axum::{body::Body, response::Response};
use gate::graph::{HeaderValueNode, ResponseBodyNode};

pub(super) fn build_direct_response(
    status: u16,
    headers: &[HeaderValueNode],
    body: Option<&ResponseBodyNode>,
) -> Result<Response, ErrorResponse> {
    let status = StatusCode::from_u16(status)
        .map_err(|_| ErrorResponse::new(ErrorCode::GatewayDirectResponseStatusInvalid))?;

    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;

    for header in headers {
        insert_header(response.headers_mut(), header);
    }

    if let Some(body) = body {
        match body {
            ResponseBodyNode::Text(text) => {
                if !response.headers().contains_key(CONTENT_TYPE) {
                    response.headers_mut().insert(
                        CONTENT_TYPE,
                        ::http::HeaderValue::from_static("text/plain; charset=utf-8"),
                    );
                }
                *response.body_mut() = Body::from(text.clone());
            }
            ResponseBodyNode::Json(value) => {
                if !response.headers().contains_key(CONTENT_TYPE) {
                    response.headers_mut().insert(
                        CONTENT_TYPE,
                        ::http::HeaderValue::from_static("application/json"),
                    );
                }
                let payload = serde_json::to_vec(value).map_err(ErrorResponse::internal)?;
                *response.body_mut() = Body::from(payload);
            }
        }
    }

    Ok(response)
}

#[derive(Debug, Default, Clone)]
pub(super) struct ResponseHeaderMutations {
    pub(super) add: Vec<HeaderValueNode>,
    pub(super) set: Vec<HeaderValueNode>,
    pub(super) remove: Vec<String>,
}

pub(in crate::api::gateway) fn apply_response_header_mutations(
    headers: &mut HeaderMap,
    mutations: &ResponseHeaderMutations,
) {
    for name in &mutations.remove {
        headers.remove(name);
    }
    for header in &mutations.add {
        append_header(headers, header);
    }
    for header in &mutations.set {
        insert_header(headers, header);
    }
}

pub(in crate::api::gateway) fn apply_gateway_headers(
    headers: &mut HeaderMap,
    additions: &HeaderMap,
) {
    for (name, value) in additions {
        headers.insert(name, value.clone());
    }
}

pub(in crate::api::gateway) fn strip_internal_context_response(headers: &mut HeaderMap) {
    headers.remove(&INTERNAL_CONTEXT_HEADER);
}
