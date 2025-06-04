use crate::err::{ErrorResponse, HttpError};
use actix_web::{web::Payload, HttpRequest, HttpResponse};
use gate::Service;

pub async fn handler(
    req: &HttpRequest,
    stream: Payload,
    uri: &str,
    _svc: &Service,
) -> Result<HttpResponse, ErrorResponse> {
    let res = actix_ws_proxy::start(req, uri.to_string(), stream).await;
    match res {
        Ok(response) => Ok(response),
        Err(e) => Err(ErrorResponse::from(HttpError::InternalServerError(
            e.to_string(),
        ))),
    }
}
