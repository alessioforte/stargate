mod http;
mod ws;

use std::net::IpAddr;

use crate::act::{access_control, validate_api_key};
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::etc::ext::RequestExt;
use crate::etc::jwt::jwt_config;
use actix_web::{HttpRequest, HttpResponse, web::Payload};
use gate::Gate;
use store::{Store, print_stats};

pub async fn handler(
    gate: actix_web::web::Data<Gate>,
    limiter: actix_web::web::Data<etc::lim::RateLimiter>,
    req: HttpRequest,
    stream: Payload,
) -> Result<HttpResponse, ErrorResponse> {
    let path = req.uri().path();
    let query = req.query_string();
    let method = req.method().clone();
    let client_ip = req.get_client_ip();

    // FIXME: handle the case where client_ip is None more gracefully
    let ip = client_ip.unwrap_or(IpAddr::from([127, 0, 0, 1]));

    // Apply rate limiting
    let decision = limiter.check_rate_limit_ip(ip).await.map_err(|e| {
        log::error!("Rate limiting error: {}", e);
        ErrorResponse::from(HttpError::InternalServerError(
            "Internal server error".to_string(),
        ))
    })?;

    if !decision.allowed {
        return Err(ErrorResponse::from(HttpError::TooManyRequests(
            "Too many requests".to_string(),
        )));
    }

    let services = gate.services.read().await;
    let protocol = req.get_protocol();

    let service = match services.search(&protocol, path) {
        Some(service) => service,
        None => {
            return Err(ErrorResponse::from(HttpError::NotFound(
                "Service not found".to_string(),
            )));
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

    let store = etc::store::use_store();
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
        let session = store.get::<db::ent::Subject>(&sid).await;
        match session {
            Some(sub) => sub,
            None => {
                return Err(ErrorResponse::from(HttpError::Unauthorized(
                    "Invalid session".to_string(),
                )));
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
    if protocol == "ws" {
        return ws::handler(&req, stream, &uri, service).await;
    }

    // Otherwise, handle it as a regular HTTP request
    http::handler(&req, stream, &uri, service).await
}
