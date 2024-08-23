use crate::errors::{ErrorResponse, HttpError};
use actix_web::{web::Payload, HttpRequest, HttpResponse};

pub async fn handler(
    req: &HttpRequest,
    stream: Payload,
    uri: &String,
) -> Result<HttpResponse, ErrorResponse> {
    let res = actix_ws_proxy::start(&req, uri.clone(), stream).await;
    match res {
        Ok(response) => Ok(response),
        Err(e) => Err(ErrorResponse::from(HttpError::InternalServerError(
            e.to_string(),
        ))),
    }
}
