use std::time::Duration;

use crate::err::{ErrorResponse, HttpError};
use actix_web::{HttpRequest, HttpResponse, HttpResponseBuilder, web::Payload};
use awc::Client;
use gate::Service;

pub async fn handler(
    req: &HttpRequest,
    stream: Payload,
    uri: &String,
    svc: &Service,
) -> Result<HttpResponse, ErrorResponse> {
    let timeout = svc
        .connect_timeout
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(5));

    let client = Client::builder().timeout(timeout).finish();

    let request = client.request_from(uri, req.head()).no_decompress();

    let response = request.send_stream(stream).await.map_err(|e| {
        log::error!("Error forwarding request to backend: {}", e);
        ErrorResponse::from(HttpError::BadGateway(
            "Failed to connect to backend service".to_string(),
        ))
    })?;

    let status = response.status();
    let mut builder = HttpResponseBuilder::new(status);

    // Remove `Connection` as per
    // https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Connection#Directives
    for (name, value) in response
        .headers()
        .iter()
        .filter(|(h, _)| *h != "connection")
    {
        builder.insert_header((name, value));
    }

    Ok(builder.streaming(response))
}
