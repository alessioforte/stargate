mod http;
mod ws;

use crate::err::{ErrorResponse, HttpError};
use crate::etc::{ac::access_control, ext::RequestExt, guard};
use gate::Gate;
use gate::cfg::service::{EnvProfile, StreamingMode};

use crate::etc::{
    gate::{get_client, get_streaming_client},
    reqctx,
};

use ::http::{
    Request,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use std::{convert::Infallible, sync::Arc};
async fn handle_hyper(mut req: Request<Body>) -> Result<Response, ErrorResponse> {
    let gate = req
        .extensions()
        .get::<Arc<Gate>>()
        .cloned()
        .expect("Gate extension must be configured");

    let auth_req = auth_request(&req);
    let mut sub = guard::verify_api_key(&auth_req).await;
    if sub.is_none() {
        sub = guard::verify_jwt(&auth_req).await;
    }

    let request_id = reqctx::request_id_from(req.extensions());
    let query = req.uri().query().unwrap_or("").to_string();
    let method = req.method().clone();
    let client_ip = req.get_client_ip();
    let has_auth = sub.is_some();

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(&request_id).unwrap(),
    );

    let path = req.uri().path().to_string();
    let protocol = req.get_protocol();
    let services = gate.services.load();

    let service = match services.search(&protocol, &path) {
        Some(service) => service,
        None => {
            return Err(ErrorResponse::from(HttpError::NotFound(
                "Service not found".to_string(),
            )));
        }
    };

    let mut cost = service.cost.unwrap_or(1);
    let subpath = path.replacen(&service.path, "", 1);
    let mut auth_required = service.auth_required.unwrap_or(false);
    let mut resource = service.resource.clone();
    let mut env_profile = service.context.as_ref().and_then(|context| context.env);
    let mut streaming: Option<StreamingMode> = None;

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
                env_profile = route
                    .value
                    .context
                    .as_ref()
                    .and_then(|context| context.env)
                    .or(env_profile);
                streaming = route.value.streaming;
            }
            None => {
                return Err(ErrorResponse::from(HttpError::MethodNotAllowed(
                    "Method not allowed".to_string(),
                )));
            }
        }
    }

    if auth_required && !has_auth {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Unauthorized".to_string(),
        )));
    }

    if has_auth {
        if let Some(r) = resource {
            let pe = gate.policy_engine.load();
            let s = sub.clone().unwrap();
            let env = reqctx::build_env_from(
                req.extensions_mut(),
                env_profile.unwrap_or(EnvProfile::Geo),
            );
            let allowed = access_control(&pe, &s, &env, &r);

            if !allowed {
                return Err(ErrorResponse::from(HttpError::Forbidden(
                    "Forbidden".to_string(),
                )));
            }
        }
    }

    let limiter = gate.limiter.load();
    let mut limit_name = "default".to_string();
    let mut quota_name: Option<String> = None;
    let mut sub_key = client_ip.clone();

    if has_auth {
        let sub = sub.unwrap();
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

    let mut key = String::with_capacity(4 + sub_key.len());
    key.push_str("lim:");
    key.push_str(&sub_key);

    let decision = match limiter.check(&limit_name, &key, None).await {
        Ok(decision) => decision,
        Err(error) => {
            tracing::error!("Rate limiter error: {}", error);
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
        let retry_after = retry_after_header_value(retry_after);
        let mut res =
            ErrorResponse::from(HttpError::TooManyRequests("Too Many Requests".to_string()));
        res.insert_header("retry-after", &retry_after)
            .insert_header("x-ratelimit-limit", &limit)
            .insert_header("x-ratelimit-remaining", &remaining);
        return Err(res);
    }

    headers.insert(
        HeaderName::from_static("x-ratelimit-limit"),
        HeaderValue::from_str(&limit).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("x-ratelimit-remaining"),
        HeaderValue::from_str(&remaining).unwrap(),
    );

    if let Some(quota_name) = quota_name {
        let mut quota_key = String::with_capacity(6 + sub_key.len());
        quota_key.push_str("quota:");
        quota_key.push_str(&sub_key);
        let decision = match limiter.check(&quota_name, &quota_key, Some(cost)).await {
            Ok(decision) => decision,
            Err(error) => {
                tracing::error!("Quota limiter error: {}", error);
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
            let retry_after = retry_after_header_value(retry_after);
            let mut res = ErrorResponse::from(HttpError::TooManyRequests(
                "Quota limit exceeded".to_string(),
            ));
            res.insert_header("retry-after", &retry_after)
                .insert_header("x-quota-limit", &quota_limit)
                .insert_header("x-quota-remaining", &quota_remaining);
            return Err(res);
        }

        headers.insert(
            HeaderName::from_static("x-quota-limit"),
            HeaderValue::from_str(&quota_limit).unwrap(),
        );
        headers.insert(
            HeaderName::from_static("x-quota-remaining"),
            HeaderValue::from_str(&quota_remaining).unwrap(),
        );
    }

    let ctx = lb::RequestContext {
        client_ip: &client_ip,
        path: &path,
        method: method.as_str(),
        key: None,
    };

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

    let mut uri = format!("{}{}", upstream.base_url, subpath);
    if !query.is_empty() {
        uri.push('?');
        uri.push_str(&query);
    }

    if protocol == "ws" {
        return ws::handler(req, &uri).await;
    }

    let client = match streaming {
        Some(StreamingMode::Sse) => get_streaming_client(&service.name),
        None => get_client(&service.name),
    }
    .ok_or_else(|| {
        ErrorResponse::from(HttpError::InternalServerError(
            "HTTP client not found".to_string(),
        ))
    })?;

    http::handler(req, &headers, &uri, &client).await
}

pub async fn service(req: Request<Body>) -> Result<Response, Infallible> {
    match handle_hyper(req).await {
        Ok(response) => Ok(response),
        Err(error) => Ok(error.into_response()),
    }
}

fn auth_request(req: &Request<Body>) -> Request<()> {
    let mut auth_req = Request::builder().uri(req.uri().clone()).body(()).unwrap();
    *auth_req.headers_mut() = req.headers().clone();
    auth_req
}

fn retry_after_header_value(retry_after: std::time::Duration) -> String {
    let retry_after = match chrono::Duration::from_std(retry_after) {
        Ok(retry_after) => retry_after,
        Err(error) => {
            tracing::warn!(
                error = ?error,
                "Retry-After duration overflowed chrono::Duration; defaulting to zero"
            );
            chrono::Duration::default()
        }
    };

    tools::duration_to_string(&retry_after)
}
