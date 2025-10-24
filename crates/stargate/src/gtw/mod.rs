mod http;
mod mid;
mod ws;

use crate::err::{ErrorResponse, HttpError};
use crate::etc::{ext::RequestExt, gate::get_client, sub::Subject};
use crate::fun::access_control;
use actix_web::{
    HttpMessage, HttpRequest, HttpResponse, middleware::from_fn, web::Payload, web::ServiceConfig,
};
use gate::Gate;

pub async fn handler(
    gate: actix_web::web::Data<Gate>,
    req: HttpRequest,
    stream: Payload,
) -> Result<HttpResponse, ErrorResponse> {
    let path = req.uri().path();
    let query = req.query_string();
    let method = req.method().clone();
    let client_ip = req.get_client_ip();
    let sub = req.extensions().get::<Option<Subject>>().cloned().flatten();
    let has_auth = sub.is_some();

    // check if the service exists --------------------------------------------
    let services = gate.services.load();
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

    // check if the service has routes defined --------------------------------
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
    // ------------------------------------------------------------------------

    // if auth is required and no subject is found return unauthorized
    if auth_required && !has_auth {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Unauthorized".to_string(),
        )));
    }

    // if a resource is define and subject is found check access control
    if let Some(resource) = resource
        && let Some(subject) = sub
    {
        let policy_engine = gate.policy_engine.load();
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
        return ws::handler(&req, stream, &uri).await;
    }

    let client = get_client(&service.name).ok_or_else(|| {
        ErrorResponse::from(HttpError::InternalServerError(
            "HTTP client not found".to_string(),
        ))
    })?;

    // Otherwise, handle it as a regular HTTP request
    http::handler(&req, stream, &uri, &client).await
}

pub fn configure(cfg: &mut ServiceConfig) {
    cfg.service(
        actix_web::web::scope("")
            .wrap(from_fn(mid::middleware))
            .default_service(actix_web::web::to(handler)),
    );
}
