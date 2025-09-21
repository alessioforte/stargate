mod http;
mod ws;

use crate::act::{access_control, validate_api_key};
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::etc::ext::RequestExt;
use crate::etc::jwt::jwt_config;
use actix_web::{web::Payload, HttpRequest, HttpResponse};
use gate::Gate;
use store::Store;

pub async fn handler(
    gate: actix_web::web::Data<Gate>,
    req: HttpRequest,
    stream: Payload,
) -> Result<HttpResponse, ErrorResponse> {
    let path = req.uri().path();
    let query = req.query_string();
    let method = req.method().clone();
    let client_ip = req.peer_addr().map(|addr| addr.ip());

    let services = gate.services.read().await;

    // Check if the request is for a WebSocket connection
    let header = req.headers().get("Upgrade");
    let is_ws = header.is_some() && header.unwrap() == "websocket";
    let protocol = if is_ws { "ws" } else { "http" };

    let service = match services.search(protocol, path) {
        Some(service) => service,
        None => {
            return Err(ErrorResponse::from(HttpError::NotFound(
                "Service not found".to_string(),
            )))
        }
    };

    let subpath = path.replacen(&service.path, "", 1);
    let mut auth_required = service.auth_required.unwrap_or(false);
    let mut resource = service.resource.clone();

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
                resource = route.value.resource.clone();
            }
            None => {
                return Err(ErrorResponse::from(HttpError::NotFound(
                    "Method not allowed".to_string(),
                )));
            }
        }
    }

    let api_key = req.get_api_key();
    let api_key_sub = validate_api_key(&api_key).await;

    let subject = if let Some(subject) = api_key_sub.clone() {
        subject
    } else {
        let jwt = jwt_config();
        let token = req.get_token();
        let claims = match jwt.validate_token(&token) {
            Ok(claims) => Some(claims),
            Err(_) => None,
        };

        if auth_required && claims.is_none() {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Unauthorized".to_string(),
            )));
        }

        let claims = claims.unwrap();
        let sid = claims.sid.clone().unwrap_or_default();
        let store = etc::store::use_store();
        let session = store.get::<db::ent::Subject>(&sid).await;
        match session {
            Some(sub) => sub,
            None => {
                return Err(ErrorResponse::from(HttpError::Unauthorized(
                    "Invalid session".to_string(),
                )))
            }
        }
    };

    // if a resource is define match it against the policies
    if let Some(resource) = resource {
        let policy_engine = gate.policy_engine.read().await;
        let allowed = access_control(&policy_engine, &subject, &resource);

        if !allowed {
            return Err(ErrorResponse::from(HttpError::Forbidden(
                "Forbidden".to_string(),
            )));
        }
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
