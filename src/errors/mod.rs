pub mod types;

use crate::errors::types::{Code, ErrorCode};
use actix_web::error::{JsonPayloadError, QueryPayloadError};
use actix_web::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::fmt;

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
    BadRequest(String),
    #[error("{0}")]
    InternalServerError(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    BadGateway(String),
}

impl ErrorCode for HttpError {
    fn error_code(&self) -> Code {
        match self {
            HttpError::MissingContentType(_) => Code::MissingContentType,
            HttpError::InvalidContentType(_, _) => Code::InvalidContentType,
            HttpError::DocumentNotFound(_) => Code::DocumentNotFound,
            HttpError::MissingPayload(_) => Code::MissingPayload,
            HttpError::Payload(e) => e.error_code(),
            HttpError::Db(_) => Code::SurrealDBError,
            HttpError::Unauthorized(_) => Code::Unauthorized,
            HttpError::InternalServerError(_) => Code::InternalServerError,
            HttpError::NotFound(_) => Code::NotFound,
            HttpError::Conflict(_) => Code::Conflict,
            HttpError::BadRequest(_) => Code::BadRequest,
            HttpError::BadGateway(_) => Code::BadGateway,
        }
    }
}

impl From<HttpError> for actix_web::Error {
    fn from(err: HttpError) -> Self {
        actix_web::Error::from(ErrorResponse::from(err))
    }
}

impl From<actix_web::error::PayloadError> for HttpError {
    fn from(error: actix_web::error::PayloadError) -> Self {
        match error {
            actix_web::error::PayloadError::Incomplete(_) => {
                HttpError::Payload(PayloadError::Payload(ActixPayloadError::IncompleteError))
            }
            _ => HttpError::Payload(PayloadError::Payload(ActixPayloadError::OtherError(error))),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ActixPayloadError {
    #[error("The provided payload is incomplete and cannot be parsed")]
    IncompleteError,
    #[error(transparent)]
    OtherError(actix_web::error::PayloadError),
}

#[derive(Debug, thiserror::Error)]
pub enum PayloadError {
    #[error(transparent)]
    Payload(ActixPayloadError),
    #[error(transparent)]
    Json(JsonPayloadError),
    #[error(transparent)]
    Query(QueryPayloadError),
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
            PayloadError::Payload(e) => match e {
                ActixPayloadError::IncompleteError => Code::BadRequest,
                ActixPayloadError::OtherError(error) => match error {
                    actix_web::error::PayloadError::EncodingCorrupted => Code::Internal,
                    actix_web::error::PayloadError::Overflow => Code::PayloadTooLarge,
                    actix_web::error::PayloadError::UnknownLength => Code::Internal,
                    actix_web::error::PayloadError::Http2Payload(_) => Code::Internal,
                    actix_web::error::PayloadError::Io(_) => Code::Internal,
                    _ => todo!(),
                },
            },
            PayloadError::Json(err) => match err {
                JsonPayloadError::Overflow { .. } => Code::PayloadTooLarge,
                JsonPayloadError::ContentType => Code::UnsupportedMediaType,
                JsonPayloadError::Payload(actix_web::error::PayloadError::Overflow) => {
                    Code::PayloadTooLarge
                }
                JsonPayloadError::Payload(_) => Code::BadRequest,
                JsonPayloadError::Deserialize(_) => Code::BadRequest,
                JsonPayloadError::Serialize(_) => Code::Internal,
                _ => Code::Internal,
            },
            PayloadError::Query(err) => match err {
                QueryPayloadError::Deserialize(_) => Code::BadRequest,
                _ => Code::Internal,
            },
            PayloadError::MissingPayload => Code::MissingPayload,
            PayloadError::MalformedPayload(_) => Code::MalformedPayload,
            PayloadError::ReceivePayload(_) => Code::Internal,
        }
    }
}

impl From<JsonPayloadError> for PayloadError {
    fn from(other: JsonPayloadError) -> Self {
        match other {
            JsonPayloadError::Deserialize(e)
                if e.classify() == serde_json::error::Category::Eof
                    && e.line() == 1
                    && e.column() == 0 =>
            {
                Self::MissingPayload
            }
            JsonPayloadError::Deserialize(e)
                if e.classify() != serde_json::error::Category::Data =>
            {
                Self::MalformedPayload(e)
            }
            _ => Self::Json(other),
        }
    }
}

impl From<QueryPayloadError> for PayloadError {
    fn from(other: QueryPayloadError) -> Self {
        Self::Query(other)
    }
}

impl From<PayloadError> for actix_web::Error {
    fn from(other: PayloadError) -> Self {
        actix_web::Error::from(ErrorResponse::from(other))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    #[serde(skip)]
    pub code: StatusCode,
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
        log::error!("{}: {}", code.name(), message);
        Self {
            code: code.http(),
            message,
            error_code: code.name(),
            error_type: code.type_(),
            error_link: code.url(),
        }
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

impl actix_web::error::ResponseError for ErrorResponse {
    fn error_response(&self) -> actix_web::HttpResponse {
        let json = serde_json::to_vec(self).unwrap();
        let mut builder = actix_web::HttpResponseBuilder::new(self.status_code());
        builder.content_type("application/json");

        if self.code == StatusCode::SERVICE_UNAVAILABLE {
            builder.insert_header((actix_web::http::header::RETRY_AFTER, "10"));
        }

        builder.body(json)
    }

    fn status_code(&self) -> StatusCode {
        self.code
    }
}
