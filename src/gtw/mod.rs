mod http;
mod ws;

use crate::err::{ErrorResponse, HttpError};
use crate::etc::{ext::RequestExt, gate::get_client, sub::Subject};
use crate::fun::access_control;
use actix_web::{
    HttpMessage, HttpRequest, HttpResponse,
    http::header::{HeaderMap, HeaderName, HeaderValue},
    web::Payload,
    web::ServiceConfig,
};
use gate::Gate;

pub async fn handler(
    gate: actix_web::web::Data<Gate>,
    req: HttpRequest,
    stream: Payload,
) -> Result<HttpResponse, ErrorResponse> {
    let ex = req.extensions();
    let sub = ex.get::<Subject>();

    let query = req.query_string();
    let method = req.method().clone();
    let client_ip = req.get_client_ip();
    let has_auth = sub.is_some();

    let mut cost;
    let mut headers = HeaderMap::new();

    // Rate limit -------------------------------------------------------------
    let limiter = gate.limiter.load();
    let mut limit_name = "default".to_string();
    let mut quota_name: Option<String> = None;
    let mut sub_key = client_ip
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    if let Some(sub) = sub {
        if let Some(rate_limit) = sub.get_attr("rate_limit") {
            if let Some(rate_limit_str) = rate_limit.as_str() {
                limit_name = rate_limit_str.to_string();
            }
        }
        if let Some(quota) = sub.get_attr("quota") {
            if let Some(quota_str) = quota.as_str() {
                quota_name = Some(quota_str.to_string());
            }
        }
        sub_key = sub.id.to_string();
    }

    let key = format!("lim:{}", sub_key);

    let decision = match limiter.check(&limit_name, &key, None).await {
        Ok(decision) => decision,
        Err(e) => {
            tracing::error!("Rate limiter error: {}", e);
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                "Rate limiter error".to_string(),
            )));
        }
    };

    let limit = decision.limit.to_string();
    let remaining = decision.remaining.to_string();

    if !decision.is_allowed() {
        let retry_after = decision
            .retry_after
            .unwrap_or(std::time::Duration::from_secs(60));
        let retry_after = chrono::Duration::from_std(retry_after).unwrap_or_default();
        let retry_after = tools::duration_to_string(&retry_after);
        let mut res =
            ErrorResponse::from(HttpError::TooManyRequests("Too Many Requests".to_string()));
        res.insert_header("retry-after", &retry_after)
            .insert_header("x-ratelimit-limit", &limit)
            .insert_header("x-ratelimit-remaining", &remaining);
        return Err(res);
    }

    // append headers
    headers.insert(
        HeaderName::from_static("x-ratelimit-limit"),
        HeaderValue::from_str(&limit).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("x-ratelimit-remaining"),
        HeaderValue::from_str(&remaining).unwrap(),
    );

    // check if the service exists --------------------------------------------
    let path = req.uri().path();
    let protocol = req.get_protocol();
    let services = gate.services.load();

    let service = match services.search(&protocol, path) {
        Some(service) => service,
        None => {
            return Err(ErrorResponse::from(HttpError::NotFound(
                "Service not found".to_string(),
            )));
        }
    };

    cost = service.cost.unwrap_or(1);
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
                cost = route.value.cost.unwrap_or(cost);
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

    // Access control ---------------------------------------------------------
    // if a resource is define and subject is found check access control
    if let Some(r) = resource
        && let Some(s) = sub
    {
        let pe = gate.policy_engine.load();
        let allowed = access_control(&pe, &s, &r);

        if !allowed {
            return Err(ErrorResponse::from(HttpError::Forbidden(
                "Forbidden".to_string(),
            )));
        }
    }

    // Quota tracking ---------------------------------------------------------
    if quota_name.is_some() {
        let quota_name = quota_name.unwrap();
        let quota_key = format!("quota:{}", sub_key);
        let decision = match limiter.check(&quota_name, &quota_key, Some(cost)).await {
            Ok(decision) => decision,
            Err(e) => {
                tracing::error!("Quota limiter error: {}", e);
                return Err(ErrorResponse::from(HttpError::InternalServerError(
                    "Quota limiter error".to_string(),
                )));
            }
        };
        let quota_limit = decision.limit.to_string();
        let quota_remaining = decision.remaining.to_string();

        if !decision.is_allowed() {
            let retry_after = decision
                .retry_after
                .unwrap_or(std::time::Duration::from_secs(60));
            let retry_after = chrono::Duration::from_std(retry_after).unwrap();
            let retry_after = tools::duration_to_string(&retry_after);
            let mut res = ErrorResponse::from(HttpError::TooManyRequests(
                "Quota limit exceeded".to_string(),
            ));
            res.insert_header("retry-after", &retry_after)
                .insert_header("x-quota-limit", &quota_limit)
                .insert_header("x-quota-remaining", &quota_remaining);

            return Err(res);
        }

        // append headers
        headers.insert(
            HeaderName::from_static("x-quota-limit"),
            HeaderValue::from_str(&quota_limit).unwrap(),
        );
        headers.insert(
            HeaderName::from_static("x-quota-remaining"),
            HeaderValue::from_str(&quota_remaining).unwrap(),
        );
    }

    // Load balancing ---------------------------------------------------------
    // create the request context for the load balancer
    let ctx = lb::RequestContext {
        client_ip,
        path: path.to_string(),
        method: method.as_str().to_string(),
        key: None,
    };
    // get the load balancer and select an upstream
    let lb = service.lb.as_ref().ok_or_else(|| {
        ErrorResponse::from(HttpError::InternalServerError(
            "Load balancer not configured".to_string(),
        ))
    })?;

    let upstream = lb.select(&ctx).ok_or_else(|| {
        ErrorResponse::from(HttpError::ServiceUnavailable(
            "No healthy upstream available".to_string(),
        ))
    })?;

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
    http::handler(&req, stream, &headers, &uri, &client).await
}

pub fn configure(cfg: &mut ServiceConfig) {
    cfg.service(actix_web::web::scope("").default_service(actix_web::web::to(handler)));
}
