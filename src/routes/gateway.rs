use crate::errors::{ErrorResponse, HttpError};
use actix_web::{http, route, HttpRequest, HttpResponse, HttpResponseBuilder};
use reqwest::{Client, Method};

use crate::actions::get_token_from_request;
use crate::data::{config::Service, AppData, Gate};
use crate::modules::auth::validate_token;

#[route(
    "/{tail:.*}",
    method = "GET",
    method = "POST",
    method = "PUT",
    method = "DELETE",
    method = "HEAD",
    method = "OPTIONS",
    method = "CONNECT",
    method = "PATCH",
    method = "TRACE"
)]
pub async fn handle_request(
    data: AppData,
    gate: Gate,
    req: HttpRequest,
) -> Result<HttpResponse, ErrorResponse> {
    let path = req.uri().path();
    let query = req.query_string();
    let method = req.method().as_str();
    let method = Method::from_bytes(method.as_bytes()).unwrap();
    let headers = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string()))
        .collect::<Vec<(String, String)>>();

    let token = get_token_from_request(&req);

    let config = gate.config.read().await;

    let service = match config.search(path) {
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

    if auth_required && validate_token(&token, &data.jwt_secret).is_err() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Unauthorized".to_string(),
        )));
    }

    // format the URI
    let mut uri = format_uri(service, path.to_string());
    if !query.is_empty() {
        uri.push_str(&format!("?{}", query));
    }

    // Build the request
    let client = Client::new();
    let mut request = client.request(method, uri);
    for (k, v) in headers {
        request = request.header(&k, &v);
    }

    // send the request
    let response = request.send().await;
    let response = match response {
        Ok(response) => response,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    };

    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await.unwrap();

    let status: http::StatusCode = http::StatusCode::from_u16(status.as_u16()).unwrap();
    let mut response = HttpResponseBuilder::new(status);
    for (k, v) in headers.iter() {
        let tuple = (k.to_string(), v.to_str().unwrap().to_string());
        response.append_header(tuple);
    }

    Ok(response.body(body))
}

fn format_uri(service: &Service, path: String) -> String {
    let mut protocol = service.protocol.clone();
    if protocol.is_empty() {
        protocol = "http".to_string();
    }
    let host = service.host.clone();
    let port = match service.port {
        Some(port) => format!(":{}", port),
        None => "".to_string(),
    };
    let path = path.replace(&service.path, "");

    format!("{}://{}{}{}", protocol, host, port, path)
}
