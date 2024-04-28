use actix_web::{
    http::{self, header::Header},
    route,
    web::Data,
    HttpRequest, HttpResponse, Responder,
};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use reqwest::{Client, Method};

use crate::config::{get_service, Config, Service};
use crate::utils::auth::validate_token;

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
pub async fn handle_request(config: Data<Config>, req: HttpRequest) -> impl Responder {
    let path = req.uri().path();
    let query = req.query_string();
    let method = req.method().as_str();
    let method = Method::from_bytes(method.as_bytes()).unwrap();
    let headers = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string()))
        .collect::<Vec<(String, String)>>();

    let auth = Authorization::<Bearer>::parse(&req);
    let token = match auth {
        Ok(auth) => auth.into_scheme().token().to_string(),
        Err(_) => "".to_string(),
    };

    let service = match get_service(path, &config.services) {
        Some(service) => service,
        None => {
            return HttpResponse::NotFound().body("Service not found");
        }
    };

    let mut auth_required = service.auth_required.unwrap_or(false);

    // check if the service has routes
    if let Some(routes) = &service.routes {
        let mut route = None;
        for r in routes {
            let mut subpath = path.replace(&service.path, "");
            if subpath == "" {
                subpath.push_str("/");
            }
            if r.path == subpath && r.method == method.as_str() {
                route = Some(r);
                break;
            }
        }
        if route.is_none() {
            return HttpResponse::NotFound().body("Route not found");
        }
        // TODO:  check if this works
        auth_required = route.unwrap().auth_required.unwrap_or(auth_required);
    }

    if auth_required && validate_token(&token).is_err() {
        return HttpResponse::Unauthorized().body("Unauthorized");
    }

    // format the URI
    let mut uri = format_uri(&service, path.to_string());
    if query != "" {
        uri.push_str(&format!("?{}", query));
    }

    // Build the request
    let client = Client::new();
    let mut request = client.request(method, uri);
    for (k, v) in headers {
        request = request.header(&k, &v);
    }

    // send the request
    let response = request.send().await.unwrap();
    let status = response.status();
    let status: http::StatusCode = http::StatusCode::from_u16(status.as_u16()).unwrap();
    let body = response.text().await.unwrap();

    HttpResponse::build(status).body(body)
}

fn format_uri(service: &Service, path: String) -> String {
    return format!(
        "{}://{}:{}{}",
        service.protocol,
        service.host,
        service.port,
        path.replace(&service.path, "")
    );
}
