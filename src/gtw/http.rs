use crate::err::{ErrorResponse, HttpError};
use actix_web::{
    HttpRequest, HttpResponse, HttpResponseBuilder, http::header::HeaderMap, web::Payload,
};

pub async fn handler(
    req: &HttpRequest,
    stream: Payload,
    headers: &HeaderMap,
    uri: &String,
    client: &awc::Client,
) -> Result<HttpResponse, ErrorResponse> {
    let request = client.request_from(uri, req.head()).no_decompress();

    let response = request.send_stream(stream).await.map_err(|e| {
        tracing::error!("Error forwarding request to backend: {}", e);
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

    // Add custom headers from the request
    for (name, value) in headers.iter() {
        builder.insert_header((name, value));
    }

    Ok(builder.streaming(response))
}
