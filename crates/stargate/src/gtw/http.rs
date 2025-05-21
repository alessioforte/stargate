use crate::err::{ErrorResponse, HttpError};
use actix_web::{web::Payload, HttpRequest, HttpResponse, HttpResponseBuilder};
use awc::Client;

pub async fn handler(
    req: &HttpRequest,
    stream: Payload,
    uri: &String,
) -> Result<HttpResponse, ErrorResponse> {
    let client = Client::default();
    let request = client.request_from(uri, req.head()).no_decompress();

    let response = request
        .send_stream(stream)
        .await
        .map_err(|e| ErrorResponse::from(HttpError::InternalServerError(e.to_string())))?;

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
