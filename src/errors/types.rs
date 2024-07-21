use actix_web;
use actix_web::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::fmt;

enum ErrorType {
    InternalError,
    InvalidRequestError,
    AuthenticationError,
}

impl fmt::Display for ErrorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ErrorType::*;

        match self {
            InternalError => write!(f, "internal"),
            InvalidRequestError => write!(f, "invalid_request"),
            AuthenticationError => write!(f, "auth"),
        }
    }
}

struct ErrCode {
    status_code: StatusCode,
    error_name: &'static str,
    error_type: ErrorType,
}

impl ErrCode {
    fn authentication(error_name: &'static str, status_code: StatusCode) -> ErrCode {
        ErrCode {
            status_code,
            error_name,
            error_type: ErrorType::AuthenticationError,
        }
    }

    fn internal(error_name: &'static str, status_code: StatusCode) -> ErrCode {
        ErrCode {
            status_code,
            error_name,
            error_type: ErrorType::InternalError,
        }
    }

    fn invalid(error_name: &'static str, status_code: StatusCode) -> ErrCode {
        ErrCode {
            status_code,
            error_name,
            error_type: ErrorType::InvalidRequestError,
        }
    }
}

#[allow(clippy::enum_variant_names)]
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum Code {
    MissingContentType,
    InvalidContentType,
    DocumentNotFound,
    MissingPayload,

    BadRequest,
    Internal,
    MalformedPayload,
    UnsupportedMediaType,
    PayloadTooLarge,

    BadParameter,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    InternalServerError,
    InvalidToken,
    MissingParameter,
    SurrealDBError,
}

impl Code {
    /// ascociate a `Code` variant to the actual ErrCode
    fn err_code(&self) -> ErrCode {
        use Code::*;

        match self {
            MissingContentType => {
                ErrCode::invalid("missing_content_type", StatusCode::UNSUPPORTED_MEDIA_TYPE)
            }
            InvalidContentType => {
                ErrCode::invalid("invalid_content_type", StatusCode::UNSUPPORTED_MEDIA_TYPE)
            }
            DocumentNotFound => ErrCode::invalid("document_not_found", StatusCode::NOT_FOUND),
            MissingPayload => ErrCode::invalid("missing_payload", StatusCode::BAD_REQUEST),

            BadRequest => ErrCode::invalid("bad_request", StatusCode::BAD_REQUEST),
            Internal => ErrCode::internal("internal", StatusCode::INTERNAL_SERVER_ERROR),
            UnsupportedMediaType => {
                ErrCode::invalid("unsupported_media_type", StatusCode::UNSUPPORTED_MEDIA_TYPE)
            }
            MalformedPayload => ErrCode::invalid("malformed_payload", StatusCode::BAD_REQUEST),
            PayloadTooLarge => ErrCode::invalid("payload_too_large", StatusCode::PAYLOAD_TOO_LARGE),
            BadParameter => ErrCode::invalid("bad_parameter", StatusCode::BAD_REQUEST),
            NotFound => ErrCode::invalid("not_found", StatusCode::NOT_FOUND),
            Conflict => ErrCode::invalid("conflict", StatusCode::CONFLICT),
            Forbidden => ErrCode::authentication("forbidden", StatusCode::FORBIDDEN),
            Unauthorized => ErrCode::authentication("unauthorized", StatusCode::UNAUTHORIZED),
            InternalServerError => {
                ErrCode::internal("internal_server_error", StatusCode::INTERNAL_SERVER_ERROR)
            }
            InvalidToken => ErrCode::authentication("invalid_api_key", StatusCode::FORBIDDEN),
            MissingParameter => ErrCode::invalid("missing_parameter", StatusCode::BAD_REQUEST),
            SurrealDBError => ErrCode::internal("surrealdb", StatusCode::INTERNAL_SERVER_ERROR),
        }
    }

    /// return the HTTP status code ascociated with the `Code`
    pub fn http(&self) -> StatusCode {
        self.err_code().status_code
    }

    /// return error name, used as error code
    pub fn name(&self) -> String {
        self.err_code().error_name.to_string()
    }

    /// return the error type
    pub fn type_(&self) -> String {
        self.err_code().error_type.to_string()
    }

    /// return the doc url ascociated with the error
    pub fn url(&self) -> String {
        format!("https://<DOCS_ENDPOINT>/errors#{}", self.name())
    }
}

pub trait ErrorCode {
    fn error_code(&self) -> Code;

    // /// returns the HTTP status code associated with the error
    // fn http_status(&self) -> StatusCode {
    //     self.error_code().http()
    // }

    // /// returns the doc url associated with the error
    // fn error_url(&self) -> String {
    //     self.error_code().url()
    // }

    // /// returns error name, used as error code
    // fn error_name(&self) -> String {
    //     self.error_code().name()
    // }

    // /// return the error type
    // fn error_type(&self) -> String {
    //     self.error_code().type_()
    // }
}
