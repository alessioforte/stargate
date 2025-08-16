mod http;
mod ws;

use crate::err::{ErrorResponse, HttpError};
use crate::etc::ext::RequestExt;
use crate::etc::jwt::jwt_config;
use actix_web::{web::Payload, HttpRequest, HttpResponse};
use gate::Gate;

pub async fn handler(
    gate: actix_web::web::Data<Gate>,
    req: HttpRequest,
    stream: Payload,
) -> Result<HttpResponse, ErrorResponse> {
    let path = req.uri().path();
    let query = req.query_string();
    let method = req.method().clone();
    let token = req.get_token();
    let client_ip = req.peer_addr().map(|addr| addr.ip());

    let config = gate.config.read().await;

    // Check if the request is for a WebSocket connection
    let header = req.headers().get("Upgrade");
    let is_ws = header.is_some() && header.unwrap() == "websocket";
    let protocol = if is_ws { "ws" } else { "http" };

    let service = match config.search(protocol, path) {
        Some(service) => service,
        None => {
            return Err(ErrorResponse::from(HttpError::NotFound(
                "Service not found".to_string(),
            )))
        }
    };

    let subpath = path.replacen(&service.path, "", 1);
    let mut auth_required = service.auth_required.unwrap_or(false);

    // check if the service has routes
    if let Some(routes) = &service.routes {
        match routes.get(method.as_str()) {
            Some(router) => {
                let route = router.at(&subpath);
                if route.is_err() {
                    return Err(ErrorResponse::from(HttpError::NotFound(
                        "Route not found".to_string(),
                    )));
                }
                let route = route.unwrap();
                auth_required = route.value.auth_required;
            }
            None => {
                return Err(ErrorResponse::from(HttpError::NotFound(
                    "Method not allowed".to_string(),
                )));
            }
        }
    }

    let jwt = jwt_config();
    if auth_required && jwt.validate_token(&token).is_err() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Unauthorized".to_string(),
        )));
    }

    // create the request context for the load balancer
    let ctx = lb::RequestContext {
        client_ip,
        path: path.to_string(),
        method: method.as_str().to_string(),
        key: None,
    };
    // get the load balancer and select an upstream
    let upstream = service.lb.as_ref().unwrap().select(&ctx).unwrap();

    // format the URI
    let mut uri = format!("{}{}", upstream.base_url, subpath);
    if !query.is_empty() {
        uri.push_str(&format!("?{}", query));
    }

    // If the request is for a WebSocket connection, handle it accordingly
    if is_ws {
        return ws::handler(&req, stream, &uri, service).await;
    }

    // Otherwise, handle it as a regular HTTP request
    http::handler(&req, stream, &uri, service).await
}
