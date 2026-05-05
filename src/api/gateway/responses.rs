use super::headers::insert_header;
use crate::err::{ErrorResponse, HttpError};
use ::http::{StatusCode, header::CONTENT_TYPE};
use axum::{body::Body, response::Response};
use gate::graph::{HeaderValueNode, ResponseBodyNode};

pub(super) fn build_direct_response(
    status: u16,
    headers: &[HeaderValueNode],
    body: Option<&ResponseBodyNode>,
) -> Result<Response, ErrorResponse> {
    let status = StatusCode::from_u16(status).map_err(|_| {
        ErrorResponse::from(HttpError::InternalServerError(
            "Invalid direct response status".to_string(),
        ))
    })?;

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
