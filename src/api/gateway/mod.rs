mod http;
mod v2;
mod ws;

use crate::err::ErrorResponse;
use crate::etc::guard;
use ::http::Request;
use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use gate::Gate;
use std::{convert::Infallible, sync::Arc};

async fn handle_hyper(req: Request<Body>) -> Result<Response, ErrorResponse> {
    let gate = req
        .extensions()
        .get::<Arc<Gate>>()
        .cloned()
        .expect("Gate extension must be configured");

    let auth_req = auth_request(&req);
    let (sub, auth_kind) = match guard::verify_api_key(&auth_req).await {
        Some(subject) => (Some(subject), Some(v2::AuthKind::ApiKey)),
        None => match guard::verify_jwt(&auth_req).await {
            Some(subject) => (Some(subject), Some(v2::AuthKind::Jwt)),
            None => (None, None),
        },
    };

    v2::handle(req, gate, auth_kind, sub).await
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
