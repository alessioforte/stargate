mod format_uri;
mod http;
mod ws;

use crate::actions::get_token_from_request;
use crate::errors::{ErrorResponse, HttpError};
use crate::modules::auth::validate_token;
use actix_web::{web::Payload, HttpRequest, HttpResponse};
pub use format_uri::format_uri;

use crate::etc::Gate;

pub async fn handler(
    gate: Gate,
    req: HttpRequest,
    stream: Payload,
) -> Result<HttpResponse, ErrorResponse> {
    let path = req.uri().path();
    let query = req.query_string();
    let method = req.method().clone();
    let token = get_token_from_request(&req);

    let config = gate.config.read().await;

    let header = req.headers().get("Upgrade");
    let is_ws = header.is_some() && header.unwrap() == "websocket";
    let protocol = if is_ws { "ws" } else { "http" };

    // let service = match config.search(path) {
    let service = match config.search(protocol, path) {
        Some(service) => service,
        None => {
            return Err(ErrorResponse::from(HttpError::NotFound(
                "Service not found".to_string(),
            )))
        }
    };

    let mut auth_required = service.auth_required.unwrap_or(false);

    // check if the service has routes
    if let Some(routes) = &service.routes {
        let mut route = None;
        for r in routes {
            let mut subpath = path.replace(&service.path, "");
            if subpath.is_empty() {
                subpath.push('/');
            }
            if r.path == subpath && r.method == method.as_str() {
                route = Some(r);
                break;
            }
        }
        if route.is_none() {
            return Err(ErrorResponse::from(HttpError::NotFound(
                "Route not found".to_string(),
            )));
        }
        auth_required = route.unwrap().auth_required.unwrap_or(auth_required);
    }

    if auth_required && validate_token(&token).is_err() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Unauthorized".to_string(),
        )));
    }

    // format the URI
    let mut uri = format_uri(service, path.to_string());
    if !query.is_empty() {
        uri.push_str(&format!("?{}", query));
    }

    if is_ws {
        return ws::handler(&req, stream, &uri).await;
    }

    http::handler(&req, stream, &uri).await
}
