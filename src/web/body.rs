//! Request-body extraction shared by the IAM API.
//!
//! Every JSON route maps an over-limit body to `request.payload_too_large`
//! instead of the syntax-error code axum's rejection would otherwise become.
//! The limit itself is the explicit `DefaultBodyLimit` the IAM planes install.

use crate::err::{ErrorCode, ErrorResponse};
use axum::{
    Json,
    extract::{FromRequest, Request},
};
use http::StatusCode;
use serde::de::DeserializeOwned;

pub(crate) async fn json_body<T>(request: Request) -> Result<T, ErrorResponse>
where
    T: DeserializeOwned,
{
    match Json::<T>::from_request(request, &()).await {
        Ok(Json(value)) => Ok(value),
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            Err(ErrorResponse::new(ErrorCode::RequestPayloadTooLarge)
                .with_message(rejection.body_text()))
        }
        Err(rejection) => {
            Err(ErrorResponse::new(ErrorCode::RequestInvalidJson)
                .with_message(rejection.body_text()))
        }
    }
}
