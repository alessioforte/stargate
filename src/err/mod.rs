pub mod types;

use http::StatusCode;
use serde::{Deserialize, Serialize};
use std::fmt;
use types::{Code, ErrorCode};
use utoipa::ToSchema;

#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("A Content-Type header is missing. Accepted values for the Content-Type header are: {}",
                .0.iter().map(|s| format!("`{}`", s)).collect::<Vec<_>>().join(", "))]
    MissingContentType(Vec<String>),
    #[error(
            "The Content-Type `{0}` is invalid. Accepted values for the Content-Type header are: {x}",
            x = .1.iter().map(|s| format!("`{}`", s)).collect::<Vec<_>>().join(", ")
        )]
    InvalidContentType(String, Vec<String>),
    #[error("Document `{0}` not found.")]
    DocumentNotFound(String),
    #[error("A {0} payload is missing.")]
    MissingPayload(String),
    #[error(transparent)]
    Payload(#[from] PayloadError),
    #[error("DB ERROR: {0}")]
    Db(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    InternalServerError(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    BadGateway(String),
    #[error("{0}")]
    TooManyRequests(String),
    #[error("{0}")]
    ServiceUnavailable(String),
    #[error("{0}")]
    MethodNotAllowed(String),
}

impl ErrorCode for HttpError {
    fn error_code(&self) -> Code {
        match self {
            HttpError::MissingContentType(_) => Code::MissingContentType,
            HttpError::InvalidContentType(_, _) => Code::InvalidContentType,
            HttpError::DocumentNotFound(_) => Code::DocumentNotFound,
            HttpError::MissingPayload(_) => Code::MissingPayload,
            HttpError::Payload(e) => e.error_code(),
            HttpError::Db(_) => Code::DBError,
            HttpError::Unauthorized(_) => Code::Unauthorized,
            HttpError::Forbidden(_) => Code::Forbidden,
            HttpError::InternalServerError(_) => Code::InternalServerError,
            HttpError::NotFound(_) => Code::NotFound,
            HttpError::Conflict(_) => Code::Conflict,
            HttpError::BadRequest(_) => Code::BadRequest,
            HttpError::BadGateway(_) => Code::BadGateway,
            HttpError::TooManyRequests(_) => Code::TooManyRequests,
            HttpError::ServiceUnavailable(_) => Code::ServiceUnavailable,
            HttpError::MethodNotAllowed(_) => Code::MethodNotAllowed,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum PayloadError {
    #[error("The json payload provided is malformed. `{0}`.")]
    MalformedPayload(serde_json::error::Error),
    #[error("A json payload is missing.")]
    MissingPayload,
    #[error("Error while receiving the playload. `{0}`.")]
    ReceivePayload(Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl ErrorCode for PayloadError {
    fn error_code(&self) -> Code {
        match self {
            PayloadError::MissingPayload => Code::MissingPayload,
            PayloadError::MalformedPayload(_) => Code::MalformedPayload,
            PayloadError::ReceivePayload(_) => Code::Internal,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    #[serde(skip)]
    pub code: StatusCode,
    #[serde(skip)]
    pub headers: Vec<(String, String)>,
    pub message: String,
    #[serde(rename = "code")]
    error_code: String,
    #[serde(rename = "type")]
    error_type: String,
    #[serde(rename = "link")]
    error_link: String,
}

impl ErrorResponse {
    pub fn from_msg(message: String, code: Code) -> Self {
        tracing::error!("{}: {}", code.name(), message);
        Self {
            code: code.http(),
            message,
            headers: Vec::new(),
            error_code: code.name().to_string(),
            error_type: code.type_().to_string(),
            error_link: code.url(),
        }
    }

    pub fn internal(error: impl fmt::Display) -> Self {
        tracing::error!("internal_server_error: {}", error);
        Self::from_msg("internal error".to_string(), Code::InternalServerError)
    }

    pub fn insert_header(&mut self, key: &str, value: &str) -> &mut Self {
        self.headers.push((key.to_string(), value.to_string()));
        self
    }

    fn body_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| {
            br#"{"message":"internal error","code":"internal_server_error","type":"internal"}"#
                .to_vec()
        })
    }
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for ErrorResponse {}

impl<T> From<T> for ErrorResponse
where
    T: std::error::Error + ErrorCode,
{
    fn from(other: T) -> Self {
        Self::from_msg(other.to_string(), other.error_code())
    }
}

impl axum::response::IntoResponse for ErrorResponse {
    fn into_response(self) -> axum::response::Response {
        use http::header::{CONTENT_TYPE, HeaderName, HeaderValue, RETRY_AFTER};

        let json = self.body_json();
        let mut builder = http::Response::builder()
            .status(self.code)
            .header(CONTENT_TYPE, "application/json");

        if self.code == StatusCode::SERVICE_UNAVAILABLE {
            builder = builder.header(RETRY_AFTER, "10");
        }

        for (key, value) in &self.headers {
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_bytes(key.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                builder = builder.header(name, val);
            }
        }

        builder
            .body(axum::body::Body::from(json))
            .unwrap_or_else(|_| {
                axum::response::Response::new(axum::body::Body::from(
                    br#"{"message":"internal error","code":"internal_server_error","type":"internal"}"#
                        .as_slice(),
                ))
            })
    }
}
