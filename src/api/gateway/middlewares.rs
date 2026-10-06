use crate::api::gateway::{
    request::RequestState,
    routing::path::{add_prefix, normalize_path, path_prefix_matches, strip_prefix},
    upstream::headers::apply_request_headers,
};
use crate::err::{ErrorCode, ErrorResponse};
use ::http::Request;
use axum::body::Body;
use gate::graph::{HttpGraph, MiddlewareNode, RouterNode};

pub fn apply_middlewares(
    graph: &HttpGraph,
    router: &RouterNode,
    req: &mut Request<Body>,
    state: &mut RequestState,
) -> Result<(), ErrorResponse> {
    for middleware_name in &router.middlewares {
        let middleware = graph.middlewares.get(middleware_name).ok_or_else(|| {
            ErrorResponse::new(ErrorCode::GatewayMiddlewareNotFound)
                .with_param("middleware", middleware_name.clone())
        })?;

        match middleware {
            MiddlewareNode::StripPrefix { prefixes } => {
                for prefix in prefixes {
                    if path_prefix_matches(&state.path, prefix) {
                        state.path = strip_prefix(&state.path, prefix);
                        break;
                    }
                }
            }
            MiddlewareNode::AddPrefix { prefix } => {
                state.path = add_prefix(&state.path, prefix);
            }
            MiddlewareNode::ReplacePathRegex {
                pattern,
                replacement,
            } => {
                let next = pattern
                    .replace_all(&state.path, replacement.as_str())
                    .to_string();
                state.path = normalize_path(&next);
            }
            MiddlewareNode::PreserveHost => {
                state.preserve_host = true;
            }
            MiddlewareNode::RequestHeaders { add, set, remove } => {
                apply_request_headers(req.headers_mut(), add, set, remove);
            }
            MiddlewareNode::ResponseHeaders { add, set, remove } => {
                state.response_headers.add.extend(add.iter().cloned());
                state.response_headers.set.extend(set.iter().cloned());
                state.response_headers.remove.extend(remove.iter().cloned());
            }
        }
    }

    Ok(())
}
