mod authentication;
mod execution;
mod lifecycle;
mod middlewares;
#[cfg(all(test, feature = "memory"))]
mod performance;
mod policies;
mod request;
mod response;
mod routing;
mod upstream;

use ::http::Request;
use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
#[cfg(all(test, feature = "memory"))]
pub(crate) use request::IngressPause;
use std::convert::Infallible;

pub async fn service(req: Request<Body>) -> Result<Response, Infallible> {
    match request::handle_hyper(req).await {
        Ok(response) => Ok(response),
        Err(error) => Ok(error.into_response()),
    }
}

#[cfg(test)]
mod tests;
